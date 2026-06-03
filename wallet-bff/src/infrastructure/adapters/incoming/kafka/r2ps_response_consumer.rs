// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::Message;
use rdkafka::consumer::{Consumer, StreamConsumer};
use std::sync::Arc;
use tracing::{error, info};

use crate::application::port::incoming::ResponseUseCase;
use crate::domain::HsmWorkerResponse;

/// Starts `thread_count` background tasks that consume from the per-instance
/// response topic and call the response use case. Tasks share the same Kafka
/// consumer group, so partitions are distributed across them.
pub fn start(
    bootstrap_servers: &str,
    group_id: &str,
    topic: &str,
    response_use_case: Arc<dyn ResponseUseCase>,
    thread_count: usize,
) {
    assert!(thread_count >= 1, "thread_count must be >= 1");

    info!(
        "Starting {} hsm-worker response consumer task(s) on topic: {}",
        thread_count, topic
    );

    for idx in 0..thread_count {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("group.id", group_id)
            .set("enable.auto.commit", "true")
            .set("auto.offset.reset", "earliest")
            .set("session.timeout.ms", "6000")
            .set("heartbeat.interval.ms", "2000")
            .set("partition.assignment.strategy", "cooperative-sticky")
            .set("fetch.wait.max.ms", "10")
            .set("max.partition.fetch.bytes", "2097152")
            .set("max.poll.interval.ms", "300000")
            .set("connections.max.idle.ms", "540000")
            .set("metadata.max.age.ms", "5000")
            .create()
            .expect("Failed to create hsm-worker response consumer");

        consumer
            .subscribe(&[topic])
            .expect("Failed to subscribe to hsm-worker response topic");

        let topic = topic.to_string();
        let response_use_case = response_use_case.clone();

        tokio::spawn(async move {
            loop {
                match consumer.recv().await {
                    Ok(msg) => {
                        let Some(payload) = msg.payload() else {
                            continue;
                        };
                        match serde_json::from_slice::<HsmWorkerResponse>(payload) {
                            Ok(response) => {
                                info!(
                                    "[task {}] Received worker response for requestId: {} on topic: {}",
                                    idx, response.request_id, topic
                                );
                                response_use_case.response_ready(response).await;
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
                    Err(e) => {
                        error!("Kafka consumer error on hsm-worker response topic: {}", e);
                    }
                }
            }
        });
    }
}
