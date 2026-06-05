// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::Message;
use rdkafka::consumer::{Consumer, StreamConsumer};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info};

use crate::application::port::outgoing::StateInitCorrelationPort;
use crate::domain::StateInitResponse;

/// Mirrors `r2ps_response_consumer::start` — 1 consumer pulling from
/// `topic` + spawn-per-message dispatch into `response_received`.
/// `worker_tasks` and `queue_depth` are unused under the spawn-per-
/// message model and kept only for API/env compatibility.
pub fn start(
    bootstrap_servers: &str,
    group_id: &str,
    topic: &str,
    correlation_port: Arc<dyn StateInitCorrelationPort>,
    _worker_tasks: usize,
    _queue_depth: usize,
) {
    info!(
        "Starting 1 consumer (spawn-per-message dispatch) on topic: {}",
        topic
    );

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("group.id", group_id)
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .set("heartbeat.interval.ms", "2000")
        .set("partition.assignment.strategy", "cooperative-sticky")
        // See hsm-worker r2ps_request_kafka_message_receiver for rationale.
        .set("fetch.wait.max.ms", "5")
        .set("max.partition.fetch.bytes", "2097152")
        .set("max.poll.interval.ms", "300000")
        .set("connections.max.idle.ms", "540000")
        .set("metadata.max.age.ms", "5000")
        // See request_sender.rs for rationale.
        .set("socket.nagle.disable", "true")
        .create()
        .expect("Failed to create state-init response consumer");

    consumer
        .subscribe(&[topic])
        .expect("Failed to subscribe to state-init response topic");

    let topic_owned = topic.to_string();
    tokio::spawn(async move {
        loop {
            match consumer.recv().await {
                Ok(msg) => {
                    let Some(payload) = msg.payload() else {
                        continue;
                    };
                    let response: StateInitResponse = match serde_json::from_slice(payload) {
                        Ok(r) => r,
                        Err(e) => {
                            error!(
                                "Failed to deserialize StateInitResponse: {} - payload: {}",
                                e,
                                String::from_utf8_lossy(payload)
                            );
                            continue;
                        }
                    };
                    let port = correlation_port.clone();
                    let topic_for_task = topic_owned.clone();
                    tokio::spawn(async move {
                        let request_id = response.request_id.clone();
                        let t = Instant::now();
                        port.response_received(response).await;
                        debug!(
                            topic = %topic_for_task,
                            request_id = %request_id,
                            response_received_us = t.elapsed().as_micros(),
                            "state-init response_received completed"
                        );
                    });
                }
                Err(e) => {
                    error!("Kafka consumer error on state-init response topic: {}", e);
                }
            }
        }
    });
}
