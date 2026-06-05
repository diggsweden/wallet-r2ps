// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::{SystemTime, UNIX_EPOCH};
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

fn build_producer(bootstrap_servers: &str, broker_address_family: &str) -> FutureProducer {
    ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("broker.address.family", broker_address_family)
        .set("message.timeout.ms", "5000")
        // acks=1 (leader-only): each BFF send is correlated against an
        // in-process oneshot keyed by UUID request_id. A leader crash that
        // loses a write surfaces as a response-correlation timeout in the
        // handler with no broker-durability requirement. Idempotence is off
        // so the produce request itself stays off the full-replication path.
        .set("acks", "1")
        // linger.ms=0: at ~150 req/s per partition (1500 rps / 10 partitions)
        // few batches fill before any linger timer fires, so any non-zero
        // linger.ms becomes a per-send latency floor on the request→worker
        // hop. Pairs with socket.nagle.disable for the same "tiny infrequent
        // per-partition traffic" pattern. batch.size still bounds bursts.
        .set("linger.ms", "0")
        .set("batch.size", "65536")
        .set("compression.type", "lz4")
        // Disable Nagle: with ~1500 req/s spread across many partitions,
        // each TCP packet is tiny and infrequent. Nagle's algorithm would
        // coalesce them at the cost of up to 40ms idle delay per send,
        // which showed up directly as broker-side kafka_lag.
        .set("socket.nagle.disable", "true")
        .create()
        .expect("Failed to create Kafka producer")
}

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
        Self {
            producer: build_producer(bootstrap_servers, broker_address_family),
            response_topic,
        }
    }
}

#[async_trait::async_trait]
impl RequestSenderPort for KafkaRequestSender {
    async fn send(&self, mut request: HsmWorkerRequest, device_id: &str) -> Result<(), String> {
        // In-place mutation avoids cloning the full request (kilobyte JWS
        // strings) just to inject the response topic the worker should reply
        // on.
        request.response_topic = self.response_topic.clone();
        let payload = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        let t_buf = now_epoch_us_bytes();
        let headers = OwnedHeaders::new().insert(Header {
            key: "t_produced_us",
            value: Some(t_buf.as_slice()),
        });
        let record = FutureRecord::to(HSM_REQUESTS_TOPIC)
            .key(device_id)
            .payload(&payload)
            .headers(headers);

        // send_result enqueues into librdkafka's internal buffer and returns
        // immediately. We deliberately drop the DeliveryFuture: the BFF's own
        // response-correlation oneshot is the authoritative success signal,
        // and awaiting broker leader ack here would put the broker RTT
        // (+ linger window) on the HTTP handler's critical path for every
        // single request, serializing throughput against broker latency.
        // QueueFull (only error returned synchronously) still surfaces
        // immediately and triggers a 500 via the caller.
        self.producer
            .send_result(record)
            .map(|_delivery_future| ())
            .map_err(|(e, _)| {
                error!("Failed to enqueue to {}: {}", HSM_REQUESTS_TOPIC, e);
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
        Self {
            producer: build_producer(bootstrap_servers, broker_address_family),
            response_topic,
        }
    }
}

#[async_trait::async_trait]
impl StateInitSenderPort for KafkaStateInitSender {
    async fn send(&self, mut request: StateInitRequest, device_id: &str) -> Result<(), String> {
        request.response_topic = self.response_topic.clone();
        let payload = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        let t_buf = now_epoch_us_bytes();
        let headers = OwnedHeaders::new().insert(Header {
            key: "t_produced_us",
            value: Some(t_buf.as_slice()),
        });
        let record = FutureRecord::to(STATE_INIT_REQUESTS_TOPIC)
            .key(device_id)
            .payload(&payload)
            .headers(headers);

        // See KafkaRequestSender::send for rationale on send_result.
        self.producer
            .send_result(record)
            .map(|_delivery_future| ())
            .map_err(|(e, _)| {
                error!("Failed to enqueue to {}: {}", STATE_INIT_REQUESTS_TOPIC, e);
                e.to_string()
            })
    }
}
