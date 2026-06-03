// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::domain::HsmWorkerRequest;
use crate::infrastructure::KafkaConfig;
use crossbeam_channel::{Sender, bounded};
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::{ClientConfig, Message};
use serde_json::from_slice;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::Duration;
use tracing::{debug, error, warn};

pub struct WorkerRequestKafkaReceiver {
    worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync>,
    running: Arc<AtomicBool>,
}

impl WorkerRequestKafkaReceiver {
    pub fn new(
        worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync>,
        running: Arc<AtomicBool>,
    ) -> WorkerRequestKafkaReceiver {
        WorkerRequestKafkaReceiver {
            worker_use_case,
            running,
        }
    }

    /// Spawns 1 consumer thread polling `hsm-requests` plus `num_workers`
    /// worker threads draining per-worker bounded channels.
    ///
    /// The consumer dispatches each message to worker `partition_id %
    /// num_workers`, so all messages from a given partition land on the
    /// same worker in arrival order. That preserves the per-partition
    /// ordering the session FSM relies on (see the FSM table in
    /// `session_state_memory_cache.rs::next_state`).
    ///
    /// Returns `num_workers + 1` handles. On shutdown (`running == false`)
    /// the consumer drops its senders and the workers exit when their
    /// receivers see the channel close.
    pub fn start(
        &self,
        config: Arc<KafkaConfig>,
        num_workers: usize,
        queue_depth: usize,
    ) -> Vec<JoinHandle<()>> {
        assert!(num_workers >= 1, "num_workers must be >= 1");
        assert!(queue_depth >= 1, "queue_depth must be >= 1");

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(num_workers + 1);
        let mut senders: Vec<Sender<HsmWorkerRequest>> = Vec::with_capacity(num_workers);

        for worker_idx in 0..num_workers {
            let (tx, rx) = bounded::<HsmWorkerRequest>(queue_depth);
            senders.push(tx);
            let use_case = self.worker_use_case.clone();
            handles.push(spawn(move || {
                debug!("hsm-requests worker {} started", worker_idx);
                while let Ok(req) = rx.recv() {
                    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        use_case.execute(req)
                    }));
                    match outcome {
                        Ok(Ok(request_id)) => {
                            debug!("HsmWorkerRequest {} processed", request_id);
                        }
                        Ok(Err(err)) => {
                            error!("execute() error: {:?}", err);
                        }
                        Err(_) => {
                            error!(
                                "panic in hsm-requests worker {} — continuing",
                                worker_idx
                            );
                        }
                    }
                }
                debug!("hsm-requests worker {} channel closed; exiting", worker_idx);
            }));
        }

        let running = self.running.clone();
        let instance_id = config.group_instance_id.clone();
        let consumer_handle = spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set("group.instance.id", &instance_id)
                // Cooperative-sticky combines two concepts: sticky assignment
                // (minimizing partition movement) and cooperative
                // rebalancing (incremental, non-blocking rebalances).
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                .set("fetch.wait.max.ms", "50")
                .set("session.timeout.ms", "6000") // Default: 45000ms
                .set("heartbeat.interval.ms", "2000") // Default: 3000ms
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                .create()
                .expect("Consumer creation failed");

            consumer
                .subscribe(&["hsm-requests"])
                .expect("Failed to subscribe to hsm-requests");

            debug!(
                "hsm-requests consumer started ({} workers, queue_depth={})",
                num_workers, queue_depth
            );

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(100)) {
                    Some(Ok(msg)) => {
                        let payload = match msg.payload() {
                            Some(bytes) => bytes,
                            None => {
                                warn!("Empty message payload");
                                continue;
                            }
                        };

                        let hsm_worker_request: HsmWorkerRequest = match from_slice(payload) {
                            Ok(req) => req,
                            Err(e) => {
                                error!("Failed to deserialize JSON: {:?}", e);
                                error!(
                                    "Payload: {:?}",
                                    String::from_utf8_lossy(payload)
                                );
                                continue;
                            }
                        };

                        let partition = msg.partition();
                        let worker_idx = partition_to_worker(partition, num_workers);
                        // Bounded send: blocks when the worker is saturated.
                        // This is the back-pressure mechanism — it pauses the
                        // consumer's poll loop until the worker drains.
                        if let Err(e) = senders[worker_idx].send(hsm_worker_request) {
                            error!(
                                "dispatch to worker {} failed (channel closed): {}",
                                worker_idx, e
                            );
                        }
                    }
                    Some(Err(e)) => {
                        error!("Kafka error on hsm-requests: {}", e);
                    }
                    None => {
                        // No message available; loop and poll again
                    }
                }
            }

            debug!("hsm-requests consumer unsubscribing");
            consumer.unsubscribe();
            drop(consumer);
            // Dropping the senders here signals the workers to exit.
            drop(senders);
            debug!("hsm-requests consumer shutdown complete");
        });
        handles.push(consumer_handle);
        handles
    }
}

/// Map a Kafka `partition_id` to a stable worker index in `0..num_workers`.
/// Uses `unsigned_abs` to tolerate the theoretical negative-partition edge
/// case rdkafka exposes; in practice partitions are non-negative.
pub(crate) fn partition_to_worker(partition: i32, num_workers: usize) -> usize {
    (partition.unsigned_abs() as usize) % num_workers
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn partition_to_worker_is_stable_modulo() {
        // Same input ⇒ same output.
        assert_eq!(partition_to_worker(7, 4), partition_to_worker(7, 4));
        // Modulo distribution.
        for p in 0..16i32 {
            assert_eq!(partition_to_worker(p, 4), (p as usize) % 4);
        }
        // Single worker absorbs everything.
        for p in 0..100i32 {
            assert_eq!(partition_to_worker(p, 1), 0);
        }
        // Negative partition does not panic.
        assert_eq!(partition_to_worker(-1, 4), 1);
        assert_eq!(partition_to_worker(-4, 4), 0);
    }

    /// Simulate the dispatch step over N channels and verify per-partition
    /// arrival order is strictly preserved (offset monotonic per partition).
    #[test]
    fn per_partition_ordering_is_preserved() {
        const N: usize = 4;
        let mut senders = Vec::with_capacity(N);
        let mut receivers = Vec::with_capacity(N);
        for _ in 0..N {
            let (tx, rx) = bounded::<(i32, u64)>(64);
            senders.push(tx);
            receivers.push(rx);
        }

        // Interleave 32 messages across 8 partitions, offset increases
        // monotonically within each partition.
        for offset in 0..32u64 {
            let partition = (offset % 8) as i32;
            let worker_idx = partition_to_worker(partition, N);
            senders[worker_idx]
                .send((partition, offset))
                .expect("send must succeed");
        }
        // Close all senders so each receiver loop terminates.
        senders.clear();

        for (worker_idx, rx) in receivers.into_iter().enumerate() {
            let mut last_offset_per_partition: HashMap<i32, u64> = HashMap::new();
            while let Ok((partition, offset)) = rx.recv() {
                // Worker must only receive partitions assigned to it.
                assert_eq!(
                    partition_to_worker(partition, N),
                    worker_idx,
                    "partition {} routed to wrong worker {}",
                    partition,
                    worker_idx
                );
                if let Some(prev) = last_offset_per_partition.insert(partition, offset) {
                    assert!(
                        offset > prev,
                        "out of order for partition {}: {} after {}",
                        partition,
                        offset,
                        prev
                    );
                }
            }
        }
    }
}
