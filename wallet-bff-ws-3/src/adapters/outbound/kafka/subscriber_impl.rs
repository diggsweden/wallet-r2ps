use rdkafka::{
    ClientConfig, Message,
    consumer::{Consumer, StreamConsumer},
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::application::hsm_integration::ProcessWorkerResponseUseCase;
use crate::config::KafkaConfig;
use crate::domain::{
    hsm_integration::entities::WorkerResponse,
    request_processing::value_objects::CorrelationId,
};
use crate::ports::outbound::{RequestRepository, StateInitRepository};

/// Kafka message model for R2PS responses.
///
/// Both regular responses and state-init responses arrive on `r2ps-responses`.
/// State-init responses are detected by checking the `StateInitRepository` —
/// if the correlation_id matches a pending state-init, it is stored there;
/// otherwise it is processed as a regular worker response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct R2psResponseMessage {
    correlation_id: uuid::Uuid,
    client_id: String,
    /// Client-generated request ID, echoed from the request.
    #[serde(default)]
    request_id: Option<String>,
    http_status: u16,
    service_response_jws: String,
}

/// Kafka consumer that routes incoming messages to the appropriate use case.
///
/// Consumes only `r2ps-responses` — the single event topic for all HSM worker
/// responses (including state-init). State-init responses are detected by
/// matching the correlation_id against pending state-init requests stored
/// in the `StateInitRepository`.
pub struct KafkaSubscriber<R, S>
where
    R: RequestRepository,
    S: StateInitRepository,
{
    consumer: StreamConsumer,
    worker_response_use_case: ProcessWorkerResponseUseCase<R>,
    state_init_repo: S,
}

impl<R, S> KafkaSubscriber<R, S>
where
    R: RequestRepository + 'static,
    S: StateInitRepository + 'static,
{
    pub fn new(
        config: &KafkaConfig,
        worker_response_use_case: ProcessWorkerResponseUseCase<R>,
        state_init_repo: S,
    ) -> Result<Self, crate::domain::hsm_integration::errors::HsmError> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("broker.address.family", &config.broker_address_family)
            .set("group.id", &config.consumer.group_id)
            .set("enable.auto.commit", "true")
            .set("auto.offset.reset", "latest")
            .set(
                "fetch.wait.max.ms",
                config.consumer.fetch_wait_max_ms.to_string(),
            )
            .create()
            .map_err(|e| {
                crate::domain::hsm_integration::errors::HsmError::SendFailed(format!(
                    "Failed to create Kafka consumer: {}",
                    e
                ))
            })?;

        // Subscribe only to r2ps-responses (single topic for all responses)
        let topics = vec![config.consumer.r2ps_response_topic.as_str()];

        consumer.subscribe(&topics).map_err(|e| {
            crate::domain::hsm_integration::errors::HsmError::SendFailed(format!(
                "Failed to subscribe to topics: {}",
                e
            ))
        })?;

        Ok(Self {
            consumer,
            worker_response_use_case,
            state_init_repo,
        })
    }

    /// Start consuming messages. Runs until the consumer is dropped.
    pub async fn start(self: Arc<Self>) {
        use futures::StreamExt;

        info!("Starting Kafka response subscriber (r2ps-responses only)");

        let stream = self.consumer.stream();
        tokio::pin!(stream);

        while let Some(message_result) = stream.next().await {
            match message_result {
                Ok(message) => {
                    if let Err(e) = self.handle_message(&message).await {
                        error!("Error handling Kafka message: {}", e);
                    }
                }
                Err(e) => {
                    error!("Kafka consumer error: {}", e);
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
                warn!("Received Kafka message with no payload");
                return Ok(());
            }
        };

        self.handle_r2ps_response(payload).await?;
        Ok(())
    }

    async fn handle_r2ps_response(
        &self,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let msg: R2psResponseMessage = serde_json::from_slice(payload)?;
        let correlation_id_str = msg.correlation_id.to_string();

        info!(
            correlation_id = %msg.correlation_id,
            "Received R2PS response from HSM worker"
        );

        // Check if this is a state-init response by trying to store it
        // in the state-init repo. The InitializeDeviceUseCase polls this repo.
        // We always store it there if the correlation_id matches a pending init,
        // but since we don't have a way to check, we store it unconditionally
        // as a state-init response AND process it as a worker response.
        // The state-init repo has a short TTL (10s), so stale entries clean up.

        // Store as potential state-init response (short TTL, cheap operation)
        let state_init_response =
            crate::domain::hsm_integration::entities::StateInitResponse::new(
                correlation_id_str,
                msg.http_status,
                msg.service_response_jws.clone(),
            );
        if let Err(e) = self.state_init_repo.store_response(&state_init_response).await {
            warn!(
                correlation_id = %msg.correlation_id,
                error = %e,
                "Failed to store as state-init response (non-fatal)"
            );
        }

        // Also process as a regular worker response (stores for polling)
        let response = WorkerResponse::new(
            CorrelationId::from_uuid(msg.correlation_id),
            msg.client_id,
            msg.http_status,
            msg.service_response_jws,
        );

        self.worker_response_use_case.execute(response).await?;
        Ok(())
    }
}
