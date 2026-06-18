// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::service::state_init_service::StateInitService;
use crate::domain::StateInitRequest;
use crate::infrastructure::KafkaConfig;
use crossbeam_channel::bounded;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::{ClientConfig, Message};
use serde_json::from_slice;
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

    /// Shared-pool dispatch: 1 consumer thread + `num_workers` worker
    /// threads draining one bounded channel. State-init requests are
    /// independent (each creates a new device), so no per-partition
    /// ordering is required. See `WorkerRequestKafkaReceiver::start`
    /// for the full rationale of the shared-pool model.
    pub fn start(
        &self,
        config: Arc<KafkaConfig>,
        num_workers: usize,
        queue_depth: usize,
    ) -> Vec<JoinHandle<()>> {
        assert!(num_workers >= 1, "num_workers must be >= 1");
        assert!(queue_depth >= 1, "queue_depth must be >= 1");

        let service = self.state_init_service.clone();
        let running = self.running.clone();
        // Distinguish state-init consumer's group.instance.id from the
        // request consumer's so Kafka tracks them as separate static
        // members within the same group.
        let instance_id = format!("{}-state-init", config.group_instance_id);

        // Raw bytes through the channel; workers parse. See
        // `r2ps_request_kafka_message_receiver::Item` for rationale.
        let (tx, rx) = bounded::<Vec<u8>>(queue_depth);

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(1 + num_workers);

        for worker_id in 0..num_workers {
            let rx = rx.clone();
            let svc = service.clone();
            handles.push(spawn(move || {
                debug!("state-init worker {} started", worker_id);
                while let Ok(payload) = rx.recv() {
                    let req: StateInitRequest = match from_slice(&payload) {
                        Ok(req) => req,
                        Err(e) => {
                            error!(worker_id, "Failed to deserialize state init request: {:?}", e);
                            error!("Payload: {:?}", String::from_utf8_lossy(&payload));
                            continue;
                        }
                    };
                    let outcome =
                        std::panic::catch_unwind(AssertUnwindSafe(|| svc.initialize(req)));
                    match outcome {
                        Ok(Ok(request_id)) => {
                            debug!(
                                worker_id,
                                request_id = %request_id,
                                "StateInitRequest processed"
                            );
                        }
                        Ok(Err(err)) => {
                            error!(worker_id, "initialize() error: {:?}", err);
                        }
                        Err(_) => {
                            error!("panic in state-init worker {} — continuing", worker_id);
                        }
                    }
                }
                debug!("state-init worker {} channel closed; exiting", worker_id);
            }));
        }
        drop(rx);

        let consumer_handle = spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set("group.instance.id", &instance_id)
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                // Throughput-tuned: see r2ps_request_kafka_message_receiver
                // for rationale on the fetch.* and queued.* knobs.
                .set("fetch.wait.max.ms", "50")
                .set("max.partition.fetch.bytes", "1048576")
                .set("queued.max.messages.kbytes", "524288")
                .set("session.timeout.ms", "6000")
                .set("heartbeat.interval.ms", "2000")
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                // See request_sender.rs in wallet-bff for rationale.
                .set("socket.nagle.disable", "true")
                .create()
                .expect("State init request consumer creation failed");

            consumer
                .subscribe(&["state-init-requests"])
                .expect("Failed to subscribe to state-init-requests");

            debug!(
                "state-init consumer started (shared pool, num_workers={}, queue_depth={})",
                num_workers, queue_depth
            );

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

                        if let Err(e) = tx.send(payload.to_vec()) {
                            error!("state-init dispatch failed (channel closed): {}", e);
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
            drop(tx);
            debug!("state-init consumer shutdown signalled");
        });

        handles.push(consumer_handle);
        handles
    }
}
