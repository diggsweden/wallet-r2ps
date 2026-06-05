// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::ClientContext;
use rdkafka::Statistics;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{BaseRecord, ProducerContext, ThreadedProducer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

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

/// Broker-ack histogram, populated 100% on librdkafka's poll thread by
/// `ProducerStatsContext::delivery`. Avoids the tokio scheduling delay
/// the old `tokio::spawn(DeliveryFuture)` sampling added to the
/// measurement. Reset every 2s by `spawn_broker_ack_reporter`.
static ACK_COUNT: AtomicU64 = AtomicU64::new(0);
static ACK_ERRS: AtomicU64 = AtomicU64::new(0);
static ACK_SUM_US: AtomicU64 = AtomicU64::new(0);
static ACK_MAX_US: AtomicU64 = AtomicU64::new(0);
// Bucket boundaries in µs: <1ms, <5ms, <10ms, <50ms, <100ms, <500ms, ≥500ms
static ACK_BUCKET_LT_1MS: AtomicU64 = AtomicU64::new(0);
static ACK_BUCKET_LT_5MS: AtomicU64 = AtomicU64::new(0);
static ACK_BUCKET_LT_10MS: AtomicU64 = AtomicU64::new(0);
static ACK_BUCKET_LT_50MS: AtomicU64 = AtomicU64::new(0);
static ACK_BUCKET_LT_100MS: AtomicU64 = AtomicU64::new(0);
static ACK_BUCKET_LT_500MS: AtomicU64 = AtomicU64::new(0);
static ACK_BUCKET_GE_500MS: AtomicU64 = AtomicU64::new(0);

impl ProducerContext for ProducerStatsContext {
    type DeliveryOpaque = Box<Instant>;
    fn delivery(
        &self,
        delivery_result: &rdkafka::message::DeliveryResult<'_>,
        sent_at: Box<Instant>,
    ) {
        // Runs on librdkafka's BG poll thread immediately after the
        // broker ack — no tokio scheduling delay in the measurement.
        let us = sent_at.elapsed().as_micros() as u64;
        match delivery_result {
            Ok(_) => {
                ACK_COUNT.fetch_add(1, Ordering::Relaxed);
                ACK_SUM_US.fetch_add(us, Ordering::Relaxed);
                // lock-free max
                let mut prev = ACK_MAX_US.load(Ordering::Relaxed);
                while us > prev {
                    match ACK_MAX_US.compare_exchange_weak(
                        prev,
                        us,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(p) => prev = p,
                    }
                }
                let b = if us < 1_000 {
                    &ACK_BUCKET_LT_1MS
                } else if us < 5_000 {
                    &ACK_BUCKET_LT_5MS
                } else if us < 10_000 {
                    &ACK_BUCKET_LT_10MS
                } else if us < 50_000 {
                    &ACK_BUCKET_LT_50MS
                } else if us < 100_000 {
                    &ACK_BUCKET_LT_100MS
                } else if us < 500_000 {
                    &ACK_BUCKET_LT_500MS
                } else {
                    &ACK_BUCKET_GE_500MS
                };
                b.fetch_add(1, Ordering::Relaxed);
            }
            Err((e, _)) => {
                ACK_ERRS.fetch_add(1, Ordering::Relaxed);
                warn!(error = %e, "kafka_producer_delivery_error");
            }
        }
    }
}

/// Spawns a reporter that emits one log line every 2s with the
/// histogram and resets the counters. Bucket boundaries chosen to
/// straddle the expected range (<5ms broker-ack on a healthy cluster,
/// ≥100ms when saturated).
pub fn spawn_broker_ack_reporter() {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tick.tick().await;
            let count = ACK_COUNT.swap(0, Ordering::Relaxed);
            let errs = ACK_ERRS.swap(0, Ordering::Relaxed);
            let sum = ACK_SUM_US.swap(0, Ordering::Relaxed);
            let max = ACK_MAX_US.swap(0, Ordering::Relaxed);
            let b1 = ACK_BUCKET_LT_1MS.swap(0, Ordering::Relaxed);
            let b5 = ACK_BUCKET_LT_5MS.swap(0, Ordering::Relaxed);
            let b10 = ACK_BUCKET_LT_10MS.swap(0, Ordering::Relaxed);
            let b50 = ACK_BUCKET_LT_50MS.swap(0, Ordering::Relaxed);
            let b100 = ACK_BUCKET_LT_100MS.swap(0, Ordering::Relaxed);
            let b500 = ACK_BUCKET_LT_500MS.swap(0, Ordering::Relaxed);
            let bge500 = ACK_BUCKET_GE_500MS.swap(0, Ordering::Relaxed);
            let avg = if count > 0 { sum / count } else { 0 };
            info!(
                count_2s = count,
                errors_2s = errs,
                avg_us = avg,
                max_us = max,
                lt_1ms = b1,
                lt_5ms = b5,
                lt_10ms = b10,
                lt_50ms = b50,
                lt_100ms = b100,
                lt_500ms = b500,
                ge_500ms = bge500,
                "kafka_producer_broker_ack_histogram"
            );
        }
    });
}

fn build_producer(
    bootstrap_servers: &str,
    broker_address_family: &str,
    linger_ms: u64,
    name: &'static str,
) -> ThreadedProducer<ProducerStatsContext> {
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
        // linger.ms is tuned together with the hsm-requests partition
        // count. Sparse partition fan-out (400) leaves linger windows
        // mostly empty regardless of value — so it's wired to an env
        // var (KAFKA_PRODUCER_LINGER_MS) for in-cluster experiments
        // without rebuilds. compression.type=none because per-message
        // lz4 framing+decompression dominated broker CPU at sparse
        // batches (broker_ack_us p99 was 279 ms).
        .set("linger.ms", linger_ms.to_string())
        .set("batch.size", "65536")
        .set("compression.type", "none")
        // Explicit produce-side pipelining knobs, set to rule out any
        // silent default that would limit broker-side concurrency.
        // Default with `enable.idempotence=false` is 1,000,000 (we
        // observe waitresp_cnt=0 at 97% of stats snapshots — could be
        // genuine, could be sampling artifact). Setting 100 explicitly
        // ensures librdkafka has no excuse to serialise.
        // socket.{send,receive}.buffer.bytes=0 uses the OS default,
        // which on many Linux kernels is 87380/4096. Setting 1 MiB
        // sidesteps TCP-window-based in-flight throttling.
        .set("max.in.flight.requests.per.connection", "100")
        .set("socket.send.buffer.bytes", "1048576")
        .set("socket.receive.buffer.bytes", "1048576")
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

// Per-send broker-ack sampling counters are no longer needed —
// `ProducerStatsContext::delivery` is invoked 100% by librdkafka's BG
// thread for every ack, recording directly into the histogram below.

/// User-side concurrency counters — incremented just before
/// `producer.send_result()` and decremented just after. The peak
/// value tracked separately and reset each report interval to expose
/// burst concurrency. If peak stays at 1 we have user-level
/// serialisation despite the async handler model; ≥dozens means many
/// handlers are pounding the producer in parallel and the bottleneck
/// is downstream of `send_result()`.
static IN_FLIGHT_NOW: AtomicU64 = AtomicU64::new(0);
static IN_FLIGHT_PEAK: AtomicU64 = AtomicU64::new(0);

/// Spawn once at startup; logs the peak in-flight observed since the
/// last call, then resets the peak. Intended to be invoked from
/// bootstrap so the task lives for the BFF's lifetime.
pub fn spawn_in_flight_reporter() {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tick.tick().await;
            let peak = IN_FLIGHT_PEAK.swap(0, Ordering::Relaxed);
            let now = IN_FLIGHT_NOW.load(Ordering::Relaxed);
            info!(
                in_flight_peak_2s = peak,
                in_flight_now = now,
                "kafka_producer_user_inflight"
            );
        }
    });
}

#[inline]
fn enter_send() -> u64 {
    let cur = IN_FLIGHT_NOW.fetch_add(1, Ordering::Relaxed) + 1;
    // Lock-free max via compare_exchange loop.
    let mut prev = IN_FLIGHT_PEAK.load(Ordering::Relaxed);
    while cur > prev {
        match IN_FLIGHT_PEAK.compare_exchange_weak(prev, cur, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(p) => prev = p,
        }
    }
    cur
}

#[inline]
fn exit_send() {
    IN_FLIGHT_NOW.fetch_sub(1, Ordering::Relaxed);
}

pub struct KafkaRequestSender {
    producer: ThreadedProducer<ProducerStatsContext>,
    response_topic: String,
}

impl KafkaRequestSender {
    pub fn new(
        bootstrap_servers: &str,
        broker_address_family: &str,
        linger_ms: u64,
        response_topic: String,
    ) -> Self {
        Self {
            producer: build_producer(
                bootstrap_servers,
                broker_address_family,
                linger_ms,
                "hsm-requests",
            ),
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
        // `with_opaque(Box::new(Instant::now()))` carries the send
        // timestamp through to `ProducerStatsContext::delivery`, where
        // we measure broker-ack RTT 100% with no tokio scheduling delay.
        let record = BaseRecord::with_opaque_to(HSM_REQUESTS_TOPIC, Box::new(Instant::now()))
            .key(device_id)
            .payload(&payload)
            .headers(headers);

        enter_send();
        let result = self.producer.send(record);
        exit_send();
        match result {
            Ok(()) => Ok(()),
            Err((e, _)) => {
                error!("Failed to enqueue to {}: {}", HSM_REQUESTS_TOPIC, e);
                Err(e.to_string())
            }
        }
    }
}

pub struct KafkaStateInitSender {
    producer: ThreadedProducer<ProducerStatsContext>,
    response_topic: String,
}

impl KafkaStateInitSender {
    pub fn new(
        bootstrap_servers: &str,
        broker_address_family: &str,
        linger_ms: u64,
        response_topic: String,
    ) -> Self {
        Self {
            producer: build_producer(
                bootstrap_servers,
                broker_address_family,
                linger_ms,
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
        let record =
            BaseRecord::with_opaque_to(STATE_INIT_REQUESTS_TOPIC, Box::new(Instant::now()))
                .key(device_id)
                .payload(&payload)
                .headers(headers);

        enter_send();
        let result = self.producer.send(record);
        exit_send();
        match result {
            Ok(()) => Ok(()),
            Err((e, _)) => {
                error!("Failed to enqueue to {}: {}", STATE_INIT_REQUESTS_TOPIC, e);
                Err(e.to_string())
            }
        }
    }
}
