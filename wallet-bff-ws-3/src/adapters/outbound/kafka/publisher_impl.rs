use rdkafka::{
    ClientConfig,
    producer::{FutureProducer, FutureRecord},
};
use serde::Serialize;
use std::time::Duration;

use crate::config::KafkaConfig;
use crate::domain::hsm_integration::{
    entities::{StateInitRequest, WorkerRequest},
    errors::HsmError,
};
use crate::ports::outbound::MessagePublisher;

/// Kafka message model for HSM worker requests.
///
/// Both regular service requests and state-init commands use this format
/// on the `r2ps-requests` topic. The worker distinguishes them by the
/// `context` field in the `outerRequestJws`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HsmWorkerRequestMessage {
    correlation_id: uuid::Uuid,
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_version: Option<u64>,
    outer_request_jws: String,
}

/// Kafka message model for state init requests.
///
/// Published to the same `r2ps-requests` topic with `context: "state-init"`
/// in the outer request JWS. The worker detects the context and handles it
/// as a `StateInit` operation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateInitCommandMessage {
    correlation_id: String,
    client_id: String,
    context: String,
    public_key: EcPublicJwkMessage,
}

#[derive(Debug, Serialize)]
struct EcPublicJwkMessage {
    kty: String,
    crv: String,
    x: String,
    y: String,
    kid: String,
}

/// Kafka-backed implementation of the `MessagePublisher` port.
#[derive(Clone)]
pub struct KafkaMessagePublisher {
    producer: FutureProducer,
    r2ps_request_topic: String,
}

impl KafkaMessagePublisher {
    pub fn new(config: &KafkaConfig) -> Result<Self, HsmError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("broker.address.family", &config.broker_address_family)
            .set("message.timeout.ms", "5000")
            .set("acks", &config.producer.acks)
            .set("retries", config.producer.retries.to_string())
            .set("linger.ms", config.producer.linger_ms.to_string())
            .set(
                "socket.nagle.disable",
                config.producer.socket_nagle_disable.to_string(),
            )
            .create()
            .map_err(|e| HsmError::SendFailed(format!("Failed to create Kafka producer: {}", e)))?;

        Ok(Self {
            producer,
            r2ps_request_topic: config.producer.r2ps_request_topic.clone(),
        })
    }
}

impl MessagePublisher for KafkaMessagePublisher {
    async fn publish_worker_request(&self, request: &WorkerRequest) -> Result<(), HsmError> {
        let message = HsmWorkerRequestMessage {
            correlation_id: request.correlation_id().as_uuid(),
            client_id: request.client_id().as_str().to_string(),
            request_id: request.request_id().map(|s| s.to_string()),
            state_version: request.state_version(),
            outer_request_jws: request.outer_request_jws().as_str().to_string(),
        };

        let payload =
            serde_json::to_string(&message).map_err(|e| HsmError::SendFailed(e.to_string()))?;
        let key = request.client_id().as_str().to_string();

        self.producer
            .send(
                FutureRecord::to(&self.r2ps_request_topic)
                    .key(&key)
                    .payload(&payload),
                Duration::from_secs(5),
            )
            .await
            .map_err(|(e, _)| HsmError::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn publish_state_init_request(&self, request: &StateInitRequest) -> Result<(), HsmError> {
        let message = StateInitCommandMessage {
            correlation_id: request.request_id().to_string(),
            client_id: request.client_id().to_string(),
            context: "state-init".to_string(),
            public_key: EcPublicJwkMessage {
                kty: request.public_key().kty().to_string(),
                crv: request.public_key().crv().to_string(),
                x: request.public_key().x().to_string(),
                y: request.public_key().y().to_string(),
                kid: request.public_key().kid().to_string(),
            },
        };

        let payload =
            serde_json::to_string(&message).map_err(|e| HsmError::SendFailed(e.to_string()))?;
        let key = request.client_id().to_string();

        self.producer
            .send(
                FutureRecord::to(&self.r2ps_request_topic)
                    .key(&key)
                    .payload(&payload),
                Duration::from_secs(5),
            )
            .await
            .map_err(|(e, _)| HsmError::SendFailed(e.to_string()))?;

        Ok(())
    }
}
