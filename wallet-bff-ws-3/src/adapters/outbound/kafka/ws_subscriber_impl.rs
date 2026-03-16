//! WebSocket-specific Kafka subscriber.
//!
//! Consumes from the `r2ps-responses` topic with a **unique group_id
//! per pod** so that every pod receives all messages. Each message is routed
//! to locally connected WebSocket clients via the `ClientConnectionRegistry`.
//!
//! Non-matching messages (client not connected on this pod) are dropped —
//! the main subscriber handles persistence for REST API polling.

use rdkafka::{
    ClientConfig, Message,
    consumer::{Consumer, StreamConsumer},
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::adapters::inbound::ws::{dto::WsResponseMsg, registry::ClientConnectionRegistry};
use crate::config::KafkaConfig;

/// Kafka message model for R2PS responses.
///
/// Device state is no longer included — state is managed server-side.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct R2psResponseMessage {
    correlation_id: uuid::Uuid,
    client_id: String,
    #[serde(default)]
    request_id: Option<String>,
    http_status: u16,
    service_response_jws: String,
}

/// WebSocket-specific Kafka subscriber.
///
/// Uses a unique group_id per pod instance to ensure fan-out:
/// every pod receives all messages and can route to its local clients.
///
/// Routes responses to WebSocket clients using the `client_id` echoed
/// back in the Kafka response message — no Valkey/Redis lookup needed.
pub struct WsKafkaSubscriber {
    consumer: StreamConsumer,
    registry: ClientConnectionRegistry,
}

impl WsKafkaSubscriber {
    /// Create a new WebSocket Kafka subscriber.
    ///
    /// The `group_id_prefix` is combined with a UUID to create a unique
    /// consumer group per pod, ensuring all pods receive all messages.
    pub fn new(
        config: &KafkaConfig,
        group_id_prefix: &str,
        registry: ClientConnectionRegistry,
    ) -> Result<Self, crate::domain::hsm_integration::errors::HsmError> {
        // Unique group_id per pod for fan-out
        let group_id = format!("{}-{}", group_id_prefix, uuid::Uuid::new_v4());
        info!(
            group_id = %group_id,
            "Creating WebSocket Kafka subscriber with unique group_id"
        );

        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("broker.address.family", &config.broker_address_family)
            .set("group.id", &group_id)
            .set("enable.auto.commit", "true")
            .set("auto.offset.reset", "latest")
            .set(
                "fetch.wait.max.ms",
                config.consumer.fetch_wait_max_ms.to_string(),
            )
            .create()
            .map_err(|e| {
                crate::domain::hsm_integration::errors::HsmError::SendFailed(format!(
                    "Failed to create WS Kafka consumer: {}",
                    e
                ))
            })?;

        // Only subscribe to r2ps-responses
        let topics = vec![config.consumer.r2ps_response_topic.as_str()];
        consumer.subscribe(&topics).map_err(|e| {
            crate::domain::hsm_integration::errors::HsmError::SendFailed(format!(
                "Failed to subscribe to topics: {}",
                e
            ))
        })?;

        Ok(Self { consumer, registry })
    }

    /// Start consuming messages. Runs until the consumer is dropped.
    pub async fn start(self: Arc<Self>) {
        use futures::StreamExt;

        info!("Starting WebSocket Kafka response subscriber");

        let stream = self.consumer.stream();
        tokio::pin!(stream);

        while let Some(message_result) = stream.next().await {
            match message_result {
                Ok(message) => {
                    if let Err(e) = self.handle_message(&message).await {
                        error!("Error handling WS Kafka message: {}", e);
                    }
                }
                Err(e) => {
                    error!("WS Kafka consumer error: {}", e);
                }
            }
        }
    }

    async fn handle_message(
        &self,
        message: &impl Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = match message.payload() {
            Some(p) => p,
            None => {
                warn!("Received WS Kafka message with no payload");
                return Ok(());
            }
        };

        let msg: R2psResponseMessage = serde_json::from_slice(payload)?;
        let client_id = &msg.client_id;

        // Check if this client is connected on this pod
        if !self.registry.is_connected(client_id) {
            debug!(
                client_id = %client_id,
                correlation_id = %msg.correlation_id,
                "Client not connected on this pod — dropping WS response"
            );
            return Ok(());
        }

        // Build response message
        let (status, result, error) = if msg.http_status == 200 {
            (
                "COMPLETE".to_string(),
                Some(msg.service_response_jws.clone()),
                None,
            )
        } else {
            (
                "ERROR".to_string(),
                None,
                Some(crate::adapters::inbound::ws::dto::WsResponseError {
                    message: "Request failed".to_string(),
                    http_status: msg.http_status,
                }),
            )
        };

        let response_msg = WsResponseMsg {
            request_id: msg.request_id.clone(),
            correlation_id: msg.correlation_id,
            status,
            result,
            error,
        };

        let delivered = self.registry.send_to_client(client_id, response_msg);

        if delivered {
            info!(
                client_id = %client_id,
                correlation_id = %msg.correlation_id,
                "Delivered WS response to client"
            );
        } else {
            debug!(
                client_id = %client_id,
                correlation_id = %msg.correlation_id,
                "Failed to deliver WS response — client may have disconnected"
            );
        }

        Ok(())
    }
}
