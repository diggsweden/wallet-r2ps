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
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::{Duration, Instant};
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

    /// Spawns 1 consumer task polling `hsm-requests` and lazily spawns one
    /// worker thread per Kafka partition the first time a message for that
    /// partition arrives. Each partition has its own bounded channel of
    /// `queue_depth` slots — there is NO shared dispatch queue, so a slow
    /// worker on partition A cannot stall dispatch to partition B (no
    /// head-of-line blocking across partitions).
    ///
    /// Per-partition ordering is preserved automatically: Kafka delivers a
    /// partition's messages in order, the consumer dispatches them to that
    /// partition's dedicated channel in order, and the partition's single
    /// worker drains them in order.
    ///
    /// `num_workers` is accepted for API/env compatibility but is unused
    /// under the per-partition model; the worker count equals the number
    /// of partitions assigned to this pod (typically ~16 of 100 for the
    /// hsm-requests topic across 6 hsm-worker pods).
    pub fn start(
        &self,
        config: Arc<KafkaConfig>,
        _num_workers: usize,
        queue_depth: usize,
    ) -> Vec<JoinHandle<()>> {
        assert!(queue_depth >= 1, "queue_depth must be >= 1");

        let use_case = self.worker_use_case.clone();
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
                "hsm-requests consumer started (per-partition lazy spawn, queue_depth={})",
                queue_depth
            );

            let mut partition_senders: HashMap<i32, Sender<HsmWorkerRequest>> = HashMap::new();
            let mut worker_handles: Vec<JoinHandle<()>> = Vec::new();

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

                        // Lazily create the per-partition channel + worker
                        // thread on first sight of this partition. After a
                        // cooperative-sticky rebalance moves a partition
                        // here, this fires once for that new partition.
                        let sender = partition_senders.entry(partition).or_insert_with(|| {
                            let (tx, rx) = bounded::<HsmWorkerRequest>(queue_depth);
                            let uc = use_case.clone();
                            let handle = spawn(move || {
                                debug!("hsm-requests partition {} worker started", partition);
                                while let Ok(req) = rx.recv() {
                                    let request_id_for_log = req.request_id.clone();
                                    let t = Instant::now();
                                    let outcome = std::panic::catch_unwind(AssertUnwindSafe(
                                        || uc.execute(req),
                                    ));
                                    let execute_us = t.elapsed().as_micros();
                                    match outcome {
                                        Ok(Ok(request_id)) => {
                                            debug!(
                                                partition,
                                                request_id = %request_id,
                                                execute_us,
                                                "HsmWorkerRequest processed"
                                            );
                                        }
                                        Ok(Err(err)) => {
                                            error!(
                                                partition,
                                                request_id = %request_id_for_log,
                                                execute_us,
                                                "execute() error: {:?}",
                                                err
                                            );
                                        }
                                        Err(_) => {
                                            error!(
                                                "panic in partition {} worker — continuing",
                                                partition
                                            );
                                        }
                                    }
                                }
                                debug!(
                                    "hsm-requests partition {} worker channel closed; exiting",
                                    partition
                                );
                            });
                            worker_handles.push(handle);
                            tx
                        });

                        // Bounded send: blocks only when THIS partition's
                        // worker is saturated. Other partitions continue to
                        // dispatch in parallel.
                        if let Err(e) = sender.send(hsm_worker_request) {
                            error!(
                                "dispatch to partition {} worker failed (channel closed): {}",
                                partition, e
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
            // Dropping the senders signals the per-partition workers to exit.
            drop(partition_senders);
            for h in worker_handles {
                let _ = h.join();
            }
            debug!("hsm-requests consumer shutdown complete");
        });

        vec![consumer_handle]
    }
}
