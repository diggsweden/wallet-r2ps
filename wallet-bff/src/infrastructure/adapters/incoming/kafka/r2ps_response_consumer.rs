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

const R2PS_RESPONSES_TOPIC: &str = "r2ps-responses";

/// Starts a background task that consumes from r2ps-responses and calls the response use case.
pub fn start(bootstrap_servers: &str, group_id: &str, response_use_case: Arc<dyn ResponseUseCase>) {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("group.id", group_id)
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .set("heartbeat.interval.ms", "2000")
        .set("partition.assignment.strategy", "cooperative-sticky")
        .create()
        .expect("Failed to create r2ps-responses consumer");

    consumer
        .subscribe(&[R2PS_RESPONSES_TOPIC])
        .expect("Failed to subscribe to r2ps-responses");

    info!("Starting r2ps-responses consumer");

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
                                "Received worker response for requestId: {}",
                                response.request_id
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
                    error!("Kafka consumer error on r2ps-responses: {}", e);
                }
            }
        }
    });
}
