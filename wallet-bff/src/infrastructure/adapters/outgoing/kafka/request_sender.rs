// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::error;

fn now_epoch_us_bytes() -> Vec<u8> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
        .to_string()
        .into_bytes()
}

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
            // acks=1 (leader-only): each BFF send is awaited synchronously and
            // the request body is idempotent via its UUID request_id (a
            // duplicate arrives at the worker, gets processed; the duplicate
            // response is dropped by the BFF correlation lookup with no
            // visible effect). The lost durability on broker-leader crash
            // surfaces as a client-side timeout and retry. Dropping
            // enable.idempotence avoids the full-replication ack on the
            // critical path of every single request.
            .set("acks", "1")
            .set("linger.ms", "5")
            .set("batch.size", "65536")
            .set("compression.type", "lz4")
            // Disable Nagle: with ~1500 req/s spread across hundreds of
            // partitions, each TCP packet is tiny and infrequent.
            // Nagle's algorithm would coalesce them at the cost of up to
            // 40ms idle delay per send, which showed up directly as
            // broker-side kafka_lag.
            .set("socket.nagle.disable", "true")
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
        let t_buf = now_epoch_us_bytes();
        let headers = OwnedHeaders::new().insert(Header {
            key: "t_produced_us",
            value: Some(t_buf.as_slice()),
        });
        self.producer
            .send(
                FutureRecord::to(HSM_REQUESTS_TOPIC)
                    .key(device_id)
                    .payload(&payload)
                    .headers(headers),
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
            // acks=1 (leader-only): each BFF send is awaited synchronously and
            // the request body is idempotent via its UUID request_id (a
            // duplicate arrives at the worker, gets processed; the duplicate
            // response is dropped by the BFF correlation lookup with no
            // visible effect). The lost durability on broker-leader crash
            // surfaces as a client-side timeout and retry. Dropping
            // enable.idempotence avoids the full-replication ack on the
            // critical path of every single request.
            .set("acks", "1")
            .set("linger.ms", "5")
            .set("batch.size", "65536")
            .set("compression.type", "lz4")
            // Disable Nagle: with ~1500 req/s spread across hundreds of
            // partitions, each TCP packet is tiny and infrequent.
            // Nagle's algorithm would coalesce them at the cost of up to
            // 40ms idle delay per send, which showed up directly as
            // broker-side kafka_lag.
            .set("socket.nagle.disable", "true")
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
        let t_buf = now_epoch_us_bytes();
        let headers = OwnedHeaders::new().insert(Header {
            key: "t_produced_us",
            value: Some(t_buf.as_slice()),
        });
        self.producer
            .send(
                FutureRecord::to(STATE_INIT_REQUESTS_TOPIC)
                    .key(device_id)
                    .payload(&payload)
                    .headers(headers),
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
