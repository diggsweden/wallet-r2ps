// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::service::state_init_service::StateInitService;
use crate::domain::StateInitRequest;
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
use std::time::Duration;
use tracing::{debug, error, warn};

pub struct StateInitRequestKafkaReceiver {
    state_init_service: Arc<StateInitService>,
    running: Arc<AtomicBool>,
}

impl StateInitRequestKafkaReceiver {
    pub fn new(state_init_service: Arc<StateInitService>, running: Arc<AtomicBool>) -> Self {
        Self {
            state_init_service,
            running,
        }
    }

    /// Same per-partition lazy-spawn layout as
    /// `WorkerRequestKafkaReceiver::start`: 1 consumer task, one worker
    /// thread per Kafka partition, partition-dedicated bounded channels.
    /// No cross-partition head-of-line blocking.
    pub fn start(
        &self,
        config: Arc<KafkaConfig>,
        _num_workers: usize,
        queue_depth: usize,
    ) -> Vec<JoinHandle<()>> {
        assert!(queue_depth >= 1, "queue_depth must be >= 1");

        let service = self.state_init_service.clone();
        let running = self.running.clone();
        // Distinguish state-init consumer's group.instance.id from the
        // request consumer's so Kafka tracks them as separate static
        // members within the same group.
        let instance_id = format!("{}-state-init", config.group_instance_id);

        let consumer_handle = spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set("group.instance.id", &instance_id)
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                .set("fetch.wait.max.ms", "50")
                .set("session.timeout.ms", "6000")
                .set("heartbeat.interval.ms", "2000")
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                .create()
                .expect("State init request consumer creation failed");

            consumer
                .subscribe(&["state-init-requests"])
                .expect("Failed to subscribe to state-init-requests");

            debug!(
                "state-init consumer started (per-partition lazy spawn, queue_depth={})",
                queue_depth
            );

            let mut partition_senders: HashMap<i32, Sender<StateInitRequest>> = HashMap::new();
            let mut worker_handles: Vec<JoinHandle<()>> = Vec::new();

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(100)) {
                    Some(Ok(msg)) => {
                        let payload = match msg.payload() {
                            Some(bytes) => bytes,
                            None => {
                                warn!("Empty state init request payload");
                                continue;
                            }
                        };

                        let req: StateInitRequest = match from_slice(payload) {
                            Ok(req) => req,
                            Err(e) => {
                                error!("Failed to deserialize state init request: {:?}", e);
                                error!(
                                    "Payload: {:?}",
                                    String::from_utf8_lossy(payload)
                                );
                                continue;
                            }
                        };

                        let partition = msg.partition();

                        let sender = partition_senders.entry(partition).or_insert_with(|| {
                            let (tx, rx) = bounded::<StateInitRequest>(queue_depth);
                            let svc = service.clone();
                            let handle = spawn(move || {
                                debug!("state-init partition {} worker started", partition);
                                while let Ok(req) = rx.recv() {
                                    let outcome = std::panic::catch_unwind(AssertUnwindSafe(
                                        || svc.initialize(req),
                                    ));
                                    match outcome {
                                        Ok(Ok(request_id)) => {
                                            debug!(
                                                partition,
                                                request_id = %request_id,
                                                "StateInitRequest processed"
                                            );
                                        }
                                        Ok(Err(err)) => {
                                            error!(
                                                partition,
                                                "initialize() error: {:?}",
                                                err
                                            );
                                        }
                                        Err(_) => {
                                            error!(
                                                "panic in state-init partition {} worker — continuing",
                                                partition
                                            );
                                        }
                                    }
                                }
                                debug!(
                                    "state-init partition {} worker channel closed; exiting",
                                    partition
                                );
                            });
                            worker_handles.push(handle);
                            tx
                        });

                        if let Err(e) = sender.send(req) {
                            error!(
                                "state-init dispatch to partition {} worker failed (channel closed): {}",
                                partition, e
                            );
                        }
                    }
                    Some(Err(e)) => {
                        error!("Kafka error on state-init-requests: {}", e);
                    }
                    None => {}
                }
            }

            debug!("state-init consumer unsubscribing");
            consumer.unsubscribe();
            drop(consumer);
            drop(partition_senders);
            for h in worker_handles {
                let _ = h.join();
            }
            debug!("state-init consumer shutdown complete");
        });

        vec![consumer_handle]
    }
}
