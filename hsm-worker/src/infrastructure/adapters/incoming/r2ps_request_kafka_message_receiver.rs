// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::domain::HsmWorkerRequest;
use crate::infrastructure::KafkaConfig;
use crossbeam_channel::bounded;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::message::Headers;
use rdkafka::{ClientConfig, Message};
use serde_json::from_slice;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, warn};

fn read_t_produced_us<M: Message>(msg: &M) -> Option<u128> {
    let headers = msg.headers()?;
    for i in 0..headers.count() {
        let h = headers.get(i);
        if h.key == "t_produced_us"
            && let Some(v) = h.value
            && let Ok(s) = std::str::from_utf8(v)
            && let Ok(n) = s.parse::<u128>()
        {
            return Some(n);
        }
    }
    None
}

fn now_epoch_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
}

/// Message + timing metadata threaded through the shared dispatch channel
/// so the worker can log how long the request spent in each preceding
/// stage: in Kafka (from BFF produce to worker consume) and in our
/// in-process channel.
struct Item {
    req: HsmWorkerRequest,
    t_produced_us: Option<u128>,
    t_received: Instant,
    partition: i32,
}

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

    /// Spawns one consumer thread polling `hsm-requests` and `num_workers`
    /// dispatch threads draining a single shared bounded channel.
    ///
    /// Cross-partition order is intentionally NOT preserved: the protocol
    /// gates each state-mutating request on its own response, so a client
    /// never has more than one in-flight request that could observe a
    /// reorder. Removing per-partition serialization lets a slow HSM
    /// execute occupy one of `num_workers` slots without blocking
    /// dispatch of unrelated messages. This eliminates the
    /// consumer-poll-loop HoL that the previous per-partition bounded
    /// model produced: a single partition filling its 256-deep channel
    /// would block `sender.send()` on the consumer's only thread, and
    /// every other partition's messages would queue up in librdkafka's
    /// internal buffer until the slow partition drained.
    ///
    /// Backpressure: when in-flight reaches `queue_depth`, the consumer
    /// poll loop blocks on `tx.send()` — Kafka brokers buffer, consumer-
    /// group lag rises. That's the correct backpressure signal upstream.
    pub fn start(
        &self,
        config: Arc<KafkaConfig>,
        num_workers: usize,
        queue_depth: usize,
    ) -> Vec<JoinHandle<()>> {
        assert!(num_workers >= 1, "num_workers must be >= 1");
        assert!(queue_depth >= 1, "queue_depth must be >= 1");

        let use_case = self.worker_use_case.clone();
        let running = self.running.clone();
        let instance_id = config.group_instance_id.clone();

        let (tx, rx) = bounded::<Item>(queue_depth);

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(1 + num_workers);

        for worker_id in 0..num_workers {
            let rx = rx.clone();
            let uc = use_case.clone();
            handles.push(spawn(move || {
                debug!("hsm-requests worker {} started", worker_id);
                while let Ok(item) = rx.recv() {
                    let request_id_for_log = item.req.request_id.clone();
                    let partition = item.partition;
                    // Time the message spent transiting Kafka (from BFF
                    // .send to our consumer.poll).
                    let kafka_lag_us = item
                        .t_produced_us
                        .map(|tp| now_epoch_us().saturating_sub(tp) as i64)
                        .unwrap_or(-1);
                    // Time the message sat in the shared in-process
                    // channel before this worker dequeued it.
                    let channel_us = item.t_received.elapsed().as_micros();
                    let t = Instant::now();
                    let outcome =
                        std::panic::catch_unwind(AssertUnwindSafe(|| uc.execute(item.req)));
                    let execute_us = t.elapsed().as_micros();
                    match outcome {
                        Ok(Ok(request_id)) => {
                            debug!(
                                partition,
                                worker_id,
                                request_id = %request_id,
                                kafka_lag_us,
                                channel_us,
                                execute_us,
                                "HsmWorkerRequest processed"
                            );
                        }
                        Ok(Err(err)) => {
                            error!(
                                partition,
                                worker_id,
                                request_id = %request_id_for_log,
                                kafka_lag_us,
                                channel_us,
                                execute_us,
                                "execute() error: {:?}",
                                err
                            );
                        }
                        Err(_) => {
                            error!("panic in hsm-requests worker {} — continuing", worker_id);
                        }
                    }
                }
                debug!("hsm-requests worker {} channel closed; exiting", worker_id);
            }));
        }
        // Drop the original rx; cloned receivers held by the workers keep
        // the channel alive. The channel closes once the consumer thread
        // drops `tx` (its only sender), signalling all workers to exit.
        drop(rx);

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
                // See request_sender.rs in wallet-bff for rationale.
                .set("socket.nagle.disable", "true")
                .create()
                .expect("Consumer creation failed");

            consumer
                .subscribe(&["hsm-requests"])
                .expect("Failed to subscribe to hsm-requests");

            debug!(
                "hsm-requests consumer started (shared pool, num_workers={}, queue_depth={})",
                num_workers, queue_depth
            );

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(100)) {
                    Some(Ok(msg)) => {
                        let t_received = Instant::now();
                        let t_produced_us = read_t_produced_us(&msg);
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
                                error!("Payload: {:?}", String::from_utf8_lossy(payload));
                                continue;
                            }
                        };

                        let item = Item {
                            req: hsm_worker_request,
                            t_produced_us,
                            t_received,
                            partition: msg.partition(),
                        };
                        // Bounded send: blocks the poll loop only when the
                        // global pool is saturated (i.e. `queue_depth`
                        // requests are already in-flight). That blocking
                        // IS the upstream backpressure — broker buffers,
                        // consumer-group lag rises.
                        if let Err(e) = tx.send(item) {
                            error!("dispatch failed (channel closed): {}", e);
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
            // Drop the only Sender — closes the channel, workers exit
            // after draining whatever remains.
            drop(tx);
            debug!("hsm-requests consumer shutdown signalled");
        });

        handles.push(consumer_handle);
        handles
    }
}
