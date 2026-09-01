// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::service::state_init_service::StateInitService;
use crate::domain::StateInitRequest;
use crate::infrastructure::KafkaConfig;
use crate::infrastructure::kafka_propagation::KafkaHeaderExtractor;
use opentelemetry::global;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::{ClientConfig, Message};
use serde_json::from_slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::Duration;
use tracing::{debug, error, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;

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

    pub fn start_worker_thread(&self, config: Arc<KafkaConfig>) -> JoinHandle<()> {
        let state_init_service = self.state_init_service.clone();
        let running = self.running.clone();

        spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set(
                    "group.instance.id",
                    format!("{}-state-init", config.group_instance_id),
                )
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                .set("fetch.wait.max.ms", "500")
                .set("session.timeout.ms", "6000")
                .set("heartbeat.interval.ms", "2000")
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                .create()
                .expect("State init request consumer creation failed");

            // Subscribe to state-init-requests topic
            consumer
                .subscribe(&["state-init-requests"])
                .expect("Failed to subscribe to state-init-requests topic");

            debug!("Starting state init Kafka consumer...");

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(100)) {
                    Some(Ok(msg)) => process_message(&msg, state_init_service.as_ref()),
                    Some(Err(e)) => {
                        error!("Kafka error on state-init-requests: {}", e);
                    }
                    None => {
                        // No message available, continue polling
                    }
                }
            }

            debug!("Unsubscribing from state-init-requests...");
            consumer.unsubscribe();
            drop(consumer);
            debug!("State init consumer shutdown complete");
        })
    }
}

/// Per-message handler mirroring r2ps_request_kafka_message_receiver:
/// extract W3C tracecontext from headers so the consumer span becomes a
/// child of the bff producer's trace.
fn process_message(msg: &BorrowedMessage<'_>, state_init_service: &StateInitService) {
    let span = tracing::info_span!("process_state_init_kafka");
    let parent_ctx = global::get_text_map_propagator(|propagator| {
        propagator.extract(&KafkaHeaderExtractor(msg.headers()))
    });
    if let Err(err) = span.set_parent(parent_ctx) {
        tracing::warn!(?err, "Failed to set parent_ctx");
    }
    let _enter = span.enter();

    let payload = match msg.payload() {
        Some(bytes) => bytes,
        None => {
            warn!("Empty state init request payload");
            return;
        }
    };

    let state_init_request: StateInitRequest = match from_slice(payload) {
        Ok(req) => req,
        Err(e) => {
            error!("Failed to deserialize state init request: {:?}", e);
            error!("Payload: {:?}", String::from_utf8_lossy(payload));
            return;
        }
    };

    let key = msg.key_view::<str>().unwrap();
    debug!("Received state init request: key='{:?}'", key);

    match state_init_service.initialize(state_init_request) {
        Ok(request_id) => {
            debug!("State init request {} processed successfully", request_id);
        }
        Err(err) => {
            error!("Error processing state init request: {:?}", err);
        }
    }
}
