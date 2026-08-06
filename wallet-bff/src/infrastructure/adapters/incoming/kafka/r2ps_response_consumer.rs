// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use opentelemetry::global;
use rdkafka::ClientConfig;
use rdkafka::Message;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use std::sync::Arc;
use tracing::{Span, error, info, instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::application::port::incoming::ResponseUseCase;
use crate::domain::HsmWorkerResponse;
use crate::infrastructure::kafka_propagation::KafkaHeaderExtractor;

/// Starts a background task that consumes from the per-instance response topic
/// and calls the response use case.
pub fn start(
    bootstrap_servers: &str,
    group_id: &str,
    topic: &str,
    response_use_case: Arc<dyn ResponseUseCase>,
) {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("group.id", group_id)
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .set("heartbeat.interval.ms", "2000")
        .set("partition.assignment.strategy", "cooperative-sticky")
        .create()
        .expect("Failed to create hsm-worker response consumer");

    consumer
        .subscribe(&[topic])
        .expect("Failed to subscribe to hsm-worker response topic");

    info!("Starting hsm-worker response consumer on topic: {}", topic);
    let topic = topic.to_string();

    tokio::spawn(async move {
        loop {
            match consumer.recv().await {
                Ok(msg) => process_message(&msg, &topic, response_use_case.as_ref()),
                Err(e) => {
                    error!("Kafka consumer error on hsm-worker response topic: {}", e);
                }
            }
        }
    });
}

/// Per-message handler. Extracts the W3C tracecontext header (set by
/// hsm-worker's response producer) and sets it as the parent of the
/// app span — so the bff response-handling span sits in the same trace
/// as the upstream bff request that originally produced the work.
#[instrument(skip_all, name = "consume_hsm_response")]
fn process_message(
    msg: &BorrowedMessage<'_>,
    topic: &str,
    response_use_case: &dyn ResponseUseCase,
) {
    let parent_ctx = global::get_text_map_propagator(|propagator| {
        propagator.extract(&KafkaHeaderExtractor(msg.headers()))
    });
    Span::current().set_parent(parent_ctx);

    let Some(payload) = msg.payload() else {
        return;
    };
    match serde_json::from_slice::<HsmWorkerResponse>(payload) {
        Ok(response) => {
            info!(
                "Received worker response for requestId: {} on topic: {}",
                response.request_id, topic
            );
            response_use_case.response_ready(response);
        }
        Err(e) => {
            error!(
                "Failed to deserialize HsmWorkerResponse: {} - payload: {}",
                e,
                String::from_utf8_lossy(payload)
            );
        }
    }
}
