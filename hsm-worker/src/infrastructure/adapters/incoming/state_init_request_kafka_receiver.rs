// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::service::state_init_service::StateInitService;
use crate::domain::StateInitRequest;
use crate::infrastructure::KafkaConfig;
use crate::infrastructure::adapters::incoming::r2ps_request_kafka_message_receiver::partition_to_worker;
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

    /// Same one-consumer + N-workers + partition-mod-routing layout as
    /// `WorkerRequestKafkaReceiver::start`, but dispatches to
    /// `state_init_service.initialize`. The state-init flow is keyed by
    /// the same client_id, so partition-mod routing keeps per-client
    /// ordering against the worker that will later handle the matching
    /// hsm-requests messages.
    pub fn start(
        &self,
        config: Arc<KafkaConfig>,
        num_workers: usize,
        queue_depth: usize,
    ) -> Vec<JoinHandle<()>> {
        assert!(num_workers >= 1, "num_workers must be >= 1");
        assert!(queue_depth >= 1, "queue_depth must be >= 1");

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(num_workers + 1);
        let mut senders: Vec<Sender<StateInitRequest>> = Vec::with_capacity(num_workers);

        for worker_idx in 0..num_workers {
            let (tx, rx) = bounded::<StateInitRequest>(queue_depth);
            senders.push(tx);
            let service = self.state_init_service.clone();
            handles.push(spawn(move || {
                debug!("state-init worker {} started", worker_idx);
                while let Ok(req) = rx.recv() {
                    let outcome =
                        std::panic::catch_unwind(AssertUnwindSafe(|| service.initialize(req)));
                    match outcome {
                        Ok(Ok(request_id)) => {
                            debug!("StateInitRequest {} processed", request_id);
                        }
                        Ok(Err(err)) => {
                            error!("initialize() error: {:?}", err);
                        }
                        Err(_) => {
                            error!(
                                "panic in state-init worker {} — continuing",
                                worker_idx
                            );
                        }
                    }
                }
                debug!("state-init worker {} channel closed; exiting", worker_idx);
            }));
        }

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
                "state-init consumer started ({} workers, queue_depth={})",
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
                        let worker_idx = partition_to_worker(partition, num_workers);
                        if let Err(e) = senders[worker_idx].send(req) {
                            error!(
                                "state-init dispatch to worker {} failed (channel closed): {}",
                                worker_idx, e
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
            drop(senders);
            debug!("state-init consumer shutdown complete");
        });
        handles.push(consumer_handle);
        handles
    }
}
