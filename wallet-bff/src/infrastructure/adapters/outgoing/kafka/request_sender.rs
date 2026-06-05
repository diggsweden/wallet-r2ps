// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::ClientContext;
use rdkafka::Statistics;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord, ProducerContext};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{error, info};

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

/// Context that turns on librdkafka's built-in statistics callback. Logs a
/// compact summary every `statistics.interval.ms` instead of letting the
/// full multi-KB JSON go to /dev/null. Picks the fields we actually want
/// to see in a saturation investigation:
///   * `outbuf_cnt` — messages parked in the producer queue per broker
///   * per-broker `rtt` (round-trip request time) — produce-request latency
///   * `int_latency` — time messages spent on the user→broker path before
///     being assigned to a request batch
/// All numeric percentiles in librdkafka stats are reported in microseconds.
#[derive(Default)]
pub struct ProducerStatsContext {
    name: &'static str,
}

impl ProducerStatsContext {
    fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl ClientContext for ProducerStatsContext {
    fn stats(&self, stats: Statistics) {
        info!(
            producer = self.name,
            msg_cnt = stats.msg_cnt,
            msg_size_bytes = stats.msg_size,
            tx = stats.tx,
            txmsgs = stats.txmsgs,
            replyq = stats.replyq,
            "rdkafka_producer_stats"
        );
        for (name, b) in &stats.brokers {
            let rtt_avg = b.rtt.as_ref().map(|w| w.avg).unwrap_or(0);
            let rtt_p99 = b.rtt.as_ref().map(|w| w.p99).unwrap_or(0);
            let int_avg = b.int_latency.as_ref().map(|w| w.avg).unwrap_or(0);
            let int_p99 = b.int_latency.as_ref().map(|w| w.p99).unwrap_or(0);
            let outbuf_avg = b.outbuf_latency.as_ref().map(|w| w.avg).unwrap_or(0);
            let outbuf_p99 = b.outbuf_latency.as_ref().map(|w| w.p99).unwrap_or(0);
            info!(
                producer = self.name,
                broker = %name,
                state = %b.state,
                outbuf_cnt = b.outbuf_cnt,
                outbuf_msg_cnt = b.outbuf_msg_cnt,
                waitresp_cnt = b.waitresp_cnt,
                tx = b.tx,
                txerrs = b.txerrs,
                txretries = b.txretries,
                rtt_avg_us = rtt_avg,
                rtt_p99_us = rtt_p99,
                int_latency_avg_us = int_avg,
                int_latency_p99_us = int_p99,
                outbuf_latency_avg_us = outbuf_avg,
                outbuf_latency_p99_us = outbuf_p99,
                "rdkafka_producer_broker"
            );
        }
    }
}

impl ProducerContext for ProducerStatsContext {
    type DeliveryOpaque = ();
    fn delivery(
        &self,
        _delivery_result: &rdkafka::message::DeliveryResult<'_>,
        _delivery_opaque: (),
    ) {
        // We don't use the delivery callback here — broker-ack timing is
        // sampled directly on the DeliveryFuture in the send path.
    }
}

fn build_producer(
    bootstrap_servers: &str,
    broker_address_family: &str,
    name: &'static str,
) -> FutureProducer<ProducerStatsContext> {
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
        // Emit the JSON stats blob every 2s. The stats() callback above
        // picks key fields and emits one structured log line per
        // (producer, broker) pair so we can grep "rdkafka_producer_broker"
        // and chart over time.
        .set("statistics.interval.ms", "2000")
        .create_with_context(ProducerStatsContext::new(name))
        .expect("Failed to create Kafka producer")
}

/// Sample every Nth send's broker-ack latency. With 1500 rps this gives
/// ~15 samples/sec/pod which is plenty to build a distribution without
/// flooding the log or paying for a per-send tokio::spawn.
const ACK_SAMPLE_EVERY: u64 = 100;
static REQUEST_SEND_COUNTER: AtomicU64 = AtomicU64::new(0);
static STATE_INIT_SEND_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct KafkaRequestSender {
    producer: FutureProducer<ProducerStatsContext>,
    response_topic: String,
}

impl KafkaRequestSender {
    pub fn new(
        bootstrap_servers: &str,
        broker_address_family: &str,
        response_topic: String,
    ) -> Self {
        Self {
            producer: build_producer(bootstrap_servers, broker_address_family, "hsm-requests"),
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

        let t_send = Instant::now();
        // send_result enqueues into librdkafka's internal buffer and returns
        // immediately. We deliberately drop the DeliveryFuture: the BFF's own
        // response-correlation oneshot is the authoritative success signal,
        // and awaiting broker leader ack here would put the broker RTT
        // (+ linger window) on the HTTP handler's critical path for every
        // single request, serializing throughput against broker latency.
        // QueueFull (only error returned synchronously) still surfaces
        // immediately and triggers a 500 via the caller.
        match self.producer.send_result(record) {
            Ok(delivery_future) => {
                // Sampled: every Nth send, await broker ack on a background
                // task and log the elapsed time. Splits "in producer queue"
                // from "broker accepted" — the gap our current send_us /
                // kafka_lag_us bookends don't cover.
                let n = REQUEST_SEND_COUNTER.fetch_add(1, Ordering::Relaxed);
                if n % ACK_SAMPLE_EVERY == 0 {
                    tokio::spawn(async move {
                        if delivery_future.await.is_ok() {
                            info!(
                                topic = HSM_REQUESTS_TOPIC,
                                broker_ack_us = t_send.elapsed().as_micros() as u64,
                                "kafka_broker_ack_sample"
                            );
                        }
                    });
                }
                // Non-sampled path: drop the DeliveryFuture (fire-and-forget).
                Ok(())
            }
            Err((e, _)) => {
                error!("Failed to enqueue to {}: {}", HSM_REQUESTS_TOPIC, e);
                Err(e.to_string())
            }
        }
    }
}

pub struct KafkaStateInitSender {
    producer: FutureProducer<ProducerStatsContext>,
    response_topic: String,
}

impl KafkaStateInitSender {
    pub fn new(
        bootstrap_servers: &str,
        broker_address_family: &str,
        response_topic: String,
    ) -> Self {
        Self {
            producer: build_producer(
                bootstrap_servers,
                broker_address_family,
                "state-init-requests",
            ),
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

        let t_send = Instant::now();
        // See KafkaRequestSender::send for rationale on send_result.
        match self.producer.send_result(record) {
            Ok(delivery_future) => {
                let n = STATE_INIT_SEND_COUNTER.fetch_add(1, Ordering::Relaxed);
                if n % ACK_SAMPLE_EVERY == 0 {
                    tokio::spawn(async move {
                        if delivery_future.await.is_ok() {
                            info!(
                                topic = STATE_INIT_REQUESTS_TOPIC,
                                broker_ack_us = t_send.elapsed().as_micros() as u64,
                                "kafka_broker_ack_sample"
                            );
                        }
                    });
                }
                Ok(())
            }
            Err((e, _)) => {
                error!("Failed to enqueue to {}: {}", STATE_INIT_REQUESTS_TOPIC, e);
                Err(e.to_string())
            }
        }
    }
}
