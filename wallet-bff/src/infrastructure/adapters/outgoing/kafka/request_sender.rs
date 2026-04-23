// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::Duration;
use tracing::error;

use crate::application::port::outgoing::{RequestSenderPort, StateInitSenderPort};
use crate::domain::{HsmWorkerRequest, StateInitRequest};

const HSM_REQUESTS_TOPIC: &str = "hsm-requests";
const STATE_INIT_REQUESTS_TOPIC: &str = "state-init-requests";

pub struct KafkaRequestSender {
    producer: FutureProducer,
    response_topic: String,
}

impl KafkaRequestSender {
    pub fn new(
        bootstrap_servers: &str,
        broker_address_family: &str,
        response_topic: String,
    ) -> Self {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("broker.address.family", broker_address_family)
            .set("message.timeout.ms", "5000")
            .set("acks", "all")
            .create()
            .expect("Failed to create Kafka producer");

        Self {
            producer,
            response_topic,
        }
    }
}

#[async_trait::async_trait]
impl RequestSenderPort for KafkaRequestSender {
    async fn send(&self, request: &HsmWorkerRequest, device_id: &str) -> Result<(), String> {
        let mut req = request.clone();
        req.response_topic = self.response_topic.clone();
        let payload = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        self.producer
            .send(
                FutureRecord::to(HSM_REQUESTS_TOPIC)
                    .key(device_id)
                    .payload(&payload),
                Duration::from_secs(5),
            )
            .await
            .map(|_| ())
            .map_err(|(e, _)| {
                error!("Failed to send to {}: {}", HSM_REQUESTS_TOPIC, e);
                e.to_string()
            })
    }
}

pub struct KafkaStateInitSender {
    producer: FutureProducer,
    response_topic: String,
}

impl KafkaStateInitSender {
    pub fn new(
        bootstrap_servers: &str,
        broker_address_family: &str,
        response_topic: String,
    ) -> Self {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("broker.address.family", broker_address_family)
            .set("message.timeout.ms", "5000")
            .set("acks", "all")
            .create()
            .expect("Failed to create Kafka state-init producer");

        Self {
            producer,
            response_topic,
        }
    }
}

#[async_trait::async_trait]
impl StateInitSenderPort for KafkaStateInitSender {
    async fn send(&self, request: &StateInitRequest, device_id: &str) -> Result<(), String> {
        let mut req = request.clone();
        req.response_topic = self.response_topic.clone();
        let payload = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        self.producer
            .send(
                FutureRecord::to(STATE_INIT_REQUESTS_TOPIC)
                    .key(device_id)
                    .payload(&payload),
                Duration::from_secs(5),
            )
            .await
            .map(|_| ())
            .map_err(|(e, _)| {
                error!("Failed to send to {}: {}", STATE_INIT_REQUESTS_TOPIC, e);
                e.to_string()
            })
    }
}
