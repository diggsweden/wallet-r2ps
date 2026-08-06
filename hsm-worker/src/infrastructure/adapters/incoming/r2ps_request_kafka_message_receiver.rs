// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::domain::HsmWorkerRequest;
use crate::infrastructure::KafkaConfig;
use crate::infrastructure::kafka_propagation::KafkaHeaderExtractor;
use opentelemetry::global;
use opentelemetry::metrics::Counter;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::{ClientConfig, Message};
use serde_json::from_slice;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::Duration;
use tracing::{Span, debug, error, instrument, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Lazy-init counter so the global meter provider (set by
/// `infrastructure::telemetry::init`) is in place before we build it.
/// Bumped once per Kafka request message processed.
fn requests_counter() -> &'static Counter<u64> {
    static C: OnceLock<Counter<u64>> = OnceLock::new();
    C.get_or_init(|| {
        global::meter("hsm-worker")
            .u64_counter("r2ps.kafka.requests")
            .with_description("HSM request messages received by hsm-worker")
            .build()
    })
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

    pub fn start_worker_thread(&self, config: Arc<KafkaConfig>) -> JoinHandle<()> {
        let worker_use_case = self.worker_use_case.clone();
        let running = self.running.clone();

        spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set("group.instance.id", &config.group_instance_id)
                // Cooperative-sticky combines two concepts: sticky assignment
                // (minimizing partition movement) and cooperative
                // rebalancing (incremental, non-blocking rebalances).
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                .set("fetch.wait.max.ms", "500")
                .set("session.timeout.ms", "6000") // Default: 45000ms
                .set("heartbeat.interval.ms", "2000") // Default: 3000ms
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                .set("partition.assignment.strategy", "cooperative-sticky") // Default: 300000ms
                .create()
                .expect("Consumer creation failed");

            // Subscribe to input topic
            consumer
                .subscribe(&["hsm-requests"])
                .expect("Failed to subscribe to topic");

            debug!("Starting Kafka consumer-producer pipeline...");

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(100)) {
                    Some(Ok(msg)) => process_message(&msg, worker_use_case.as_ref()),
                    Some(Err(e)) => {
                        error!("Kafka error: {}", e);
                    }
                    None => {
                        // No message available, continue polling
                    }
                }
            }
            debug!("Unsubscribing...");
            consumer.unsubscribe();
            drop(consumer);
            debug!("Consumer shutdown complete");
        })
    }
}

/// Per-message handler. `#[instrument]` creates an OTel span via the
/// tracing-opentelemetry layer (see `infrastructure::telemetry`). We
/// extract the W3C tracecontext from the message's headers (injected
/// by the bff producer) and set it as the span's parent so the
/// consumer span becomes a child of the bff request trace.
#[instrument(skip_all, name = "process_request_kafka")]
fn process_message(
    msg: &BorrowedMessage<'_>,
    worker_use_case: &(dyn WorkerRequestUseCase + Send + Sync),
) {
    requests_counter().add(1, &[]);
    let parent_ctx = global::get_text_map_propagator(|propagator| {
        propagator.extract(&KafkaHeaderExtractor(msg.headers()))
    });
    Span::current().set_parent(parent_ctx);

    let payload = match msg.payload() {
        Some(bytes) => bytes,
        None => {
            warn!("Empty message payload");
            return;
        }
    };

    let hsm_worker_request: HsmWorkerRequest = match from_slice(payload) {
        Ok(req) => req,
        Err(e) => {
            error!("Failed to deserialize JSON: {:?}", e);
            error!("Payload: {:?}", String::from_utf8_lossy(payload));
            return;
        }
    };

    let key = msg.key_view::<str>().unwrap();
    debug!("Received message: key={:?}", key);

    match worker_use_case.execute(hsm_worker_request) {
        Ok(request_id) => {
            debug!("HsmWorkerRequest {} processed successfully", request_id);
        }
        Err(err) => {
            error!("Error processing message: {:?}", err);
        }
    }
}
