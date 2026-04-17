// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::Message;
use rdkafka::consumer::{Consumer, StreamConsumer};
use std::sync::Arc;
use tracing::{error, info};

use crate::application::port::outgoing::StateInitCorrelationPort;
use crate::domain::StateInitResponse;

/// Starts a background task that consumes from the per-instance state-init
/// response topic and notifies the correlation service.
pub fn start(
    bootstrap_servers: &str,
    group_id: &str,
    topic: &str,
    correlation_port: Arc<dyn StateInitCorrelationPort>,
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
        .expect("Failed to create state-init response consumer");

    consumer
        .subscribe(&[topic])
        .expect("Failed to subscribe to state-init response topic");

    info!("Starting state-init response consumer on topic: {}", topic);
    let topic = topic.to_string();

    tokio::spawn(async move {
        loop {
            match consumer.recv().await {
                Ok(msg) => {
                    if msg.key() == Some(super::HEARTBEAT_KEY) {
                        continue;
                    }
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

                    info!(
                        "Received state-init response for requestId: {} on topic: {}",
                        response.request_id, topic
                    );

                    correlation_port.response_received(response).await;
                }
                Err(e) => {
                    error!("Kafka consumer error on state-init response topic: {}", e);
                }
            }
        }
    });
}
