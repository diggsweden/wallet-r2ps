// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::domain::HsmWorkerRequest;
use crate::infrastructure::KafkaConfig;
use crossbeam_channel::bounded;
use rdkafka::consumer::{BaseConsumer, Consumer, ConsumerContext};
use rdkafka::message::Headers;
use rdkafka::{ClientConfig, ClientContext, Message, Statistics};
use serde_json::from_slice;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

/// Context that exposes librdkafka's built-in statistics callback. Emits
/// one structured info log per stats interval per (consumer, broker) and
/// another summarising per-partition fetch state with non-trivial lag.
/// Lets us see fetch RTT, fetch session counts, per-partition
/// consumer_lag, and how full librdkafka's internal queues are — fields
/// our kafka_lag_us/channel_us bookends can't reach.
#[derive(Default)]
struct ConsumerStatsContext;

impl ClientContext for ConsumerStatsContext {
    fn stats(&self, stats: Statistics) {
        info!(
            consumer = "hsm-requests",
            rx_total = stats.rx,
            rxmsgs = stats.rxmsgs,
            rxmsg_bytes = stats.rxmsg_bytes,
            replyq = stats.replyq,
            msg_cnt = stats.msg_cnt,
            "rdkafka_consumer_stats"
        );
        for (name, b) in &stats.brokers {
            let rtt_avg = b.rtt.as_ref().map(|w| w.avg).unwrap_or(0);
            let rtt_p99 = b.rtt.as_ref().map(|w| w.p99).unwrap_or(0);
            info!(
                consumer = "hsm-requests",
                broker = %name,
                state = %b.state,
                rx_total = b.rx,
                rxbytes = b.rxbytes,
                rxerrs = b.rxerrs,
                rxcorriderrs = b.rxcorriderrs,
                rtt_avg_us = rtt_avg,
                rtt_p99_us = rtt_p99,
                outbuf_cnt = b.outbuf_cnt,
                "rdkafka_consumer_broker"
            );
        }
        // Per-partition fetch state — only when actually lagging, to keep
        // the log volume manageable across 16+ partitions per pod.
        for (topic_name, topic) in &stats.topics {
            for (pid, p) in &topic.partitions {
                if p.consumer_lag > 0 || p.fetch_state != "active" {
                    info!(
                        consumer = "hsm-requests",
                        topic = %topic_name,
                        partition = pid,
                        fetch_state = %p.fetch_state,
                        consumer_lag = p.consumer_lag,
                        next_offset = p.next_offset,
                        hi_offset = p.hi_offset,
                        rx_msgs = p.rxmsgs,
                        "rdkafka_consumer_partition"
                    );
                }
            }
        }
    }
}

impl ConsumerContext for ConsumerStatsContext {}

fn read_t_produced_us<M: Message>(msg: &M) -> Option<u128> {
    let headers = msg.headers()?;
    for i in 0..headers.count() {
        let h = headers.get(i);
        if h.key == "t_produced_us"
            && let Some(v) = h.value
            && let Ok(s) = std::str::from_utf8(v)
            && let Ok(n) = s.parse::<u128>()
        {
            return Some(n);
        }
    }
    None
}

fn now_epoch_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
}

/// Message + timing metadata threaded through the shared dispatch channel
/// so the worker can log how long the request spent in each preceding
/// stage: in Kafka (from BFF produce to worker consume) and in our
/// in-process channel.
///
/// The payload is carried as raw bytes — JSON deserialization is
/// deferred to the worker threads so it runs in parallel across
/// `num_workers` instead of serializing the single consumer poll thread.
/// Each request payload is a multi-KB JWS-bearing JSON document; doing
/// `from_slice` on the consumer thread previously capped its drain rate
/// at ~2k/sec/pod and produced librdkafka internal-queue buildup during
/// fetch bursts that surfaced as `kafka_lag_us` tail.
struct Item {
    payload: Vec<u8>,
    t_produced_us: Option<u128>,
    t_received: Instant,
    partition: i32,
}

pub struct WorkerRequestKafkaReceiver {
    worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync>,
    running: Arc<AtomicBool>,
}

impl WorkerRequestKafkaReceiver {
    pub fn new(
        worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync>,
        running: Arc<AtomicBool>,
    ) -> WorkerRequestKafkaReceiver {
        WorkerRequestKafkaReceiver {
            worker_use_case,
            running,
        }
    }

    /// Spawns one consumer thread polling `hsm-requests` and `num_workers`
    /// dispatch threads draining a single shared bounded channel.
    ///
    /// Cross-partition order is intentionally NOT preserved: the protocol
    /// gates each state-mutating request on its own response, so a client
    /// never has more than one in-flight request that could observe a
    /// reorder. Removing per-partition serialization lets a slow HSM
    /// execute occupy one of `num_workers` slots without blocking
    /// dispatch of unrelated messages. This eliminates the
    /// consumer-poll-loop HoL that the previous per-partition bounded
    /// model produced: a single partition filling its 256-deep channel
    /// would block `sender.send()` on the consumer's only thread, and
    /// every other partition's messages would queue up in librdkafka's
    /// internal buffer until the slow partition drained.
    ///
    /// Backpressure: when in-flight reaches `queue_depth`, the consumer
    /// poll loop blocks on `tx.send()` — Kafka brokers buffer, consumer-
    /// group lag rises. That's the correct backpressure signal upstream.
    pub fn start(
        &self,
        config: Arc<KafkaConfig>,
        num_workers: usize,
        queue_depth: usize,
        num_consumers: usize,
    ) -> Vec<JoinHandle<()>> {
        assert!(num_workers >= 1, "num_workers must be >= 1");
        assert!(queue_depth >= 1, "queue_depth must be >= 1");
        assert!(num_consumers >= 1, "num_consumers must be >= 1");

        let use_case = self.worker_use_case.clone();
        let running = self.running.clone();
        let instance_id = config.group_instance_id.clone();

        let (tx, rx) = bounded::<Item>(queue_depth);

        let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(num_consumers + num_workers);

        for worker_id in 0..num_workers {
            let rx = rx.clone();
            let uc = use_case.clone();
            handles.push(spawn(move || {
                debug!("hsm-requests worker {} started", worker_id);
                while let Ok(item) = rx.recv() {
                    let partition = item.partition;
                    // Time the message spent transiting Kafka (from BFF
                    // .send to our consumer.poll).
                    let kafka_lag_us = item
                        .t_produced_us
                        .map(|tp| now_epoch_us().saturating_sub(tp) as i64)
                        .unwrap_or(-1);
                    // Time the message sat in the shared in-process
                    // channel before this worker dequeued it.
                    let channel_us = item.t_received.elapsed().as_micros();

                    // Deserialize on the worker — see Item docs for rationale.
                    let t_deser = Instant::now();
                    let hsm_worker_request: HsmWorkerRequest = match from_slice(&item.payload) {
                        Ok(req) => req,
                        Err(e) => {
                            error!(
                                partition,
                                worker_id,
                                "Failed to deserialize JSON: {:?}", e
                            );
                            error!("Payload: {:?}", String::from_utf8_lossy(&item.payload));
                            continue;
                        }
                    };
                    let deser_us = t_deser.elapsed().as_micros();

                    let request_id_for_log = hsm_worker_request.request_id.clone();
                    let t = Instant::now();
                    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        uc.execute(hsm_worker_request)
                    }));
                    let execute_us = t.elapsed().as_micros();
                    match outcome {
                        Ok(Ok(request_id)) => {
                            debug!(
                                partition,
                                worker_id,
                                request_id = %request_id,
                                kafka_lag_us,
                                channel_us,
                                deser_us,
                                execute_us,
                                "HsmWorkerRequest processed"
                            );
                        }
                        Ok(Err(err)) => {
                            error!(
                                partition,
                                worker_id,
                                request_id = %request_id_for_log,
                                kafka_lag_us,
                                channel_us,
                                deser_us,
                                execute_us,
                                "execute() error: {:?}",
                                err
                            );
                        }
                        Err(_) => {
                            error!("panic in hsm-requests worker {} — continuing", worker_id);
                        }
                    }
                }
                debug!("hsm-requests worker {} channel closed; exiting", worker_id);
            }));
        }
        // Drop the original rx; cloned receivers held by the workers keep
        // the channel alive. The channel closes once all consumer threads
        // drop their `tx`, signalling all workers to exit.
        drop(rx);

        // Spawn N independent BaseConsumer poll loops. Each joins the same
        // consumer group (group.id) with a unique group.instance.id so
        // Kafka assigns each a disjoint subset of partitions. A single
        // poll loop was the throughput cap on this pod: fetch.wait.max.ms
        // stalls on idle partitions serialise on one thread.
        for consumer_idx in 0..num_consumers {
            let tx = tx.clone();
            let config = config.clone();
            let running = running.clone();
            let instance_id_full = if num_consumers == 1 {
                instance_id.clone()
            } else {
                format!("{}-c{}", instance_id, consumer_idx)
            };
            let consumer_handle = spawn(move || {
            let consumer: BaseConsumer<ConsumerStatsContext> = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set("group.instance.id", &instance_id_full)
                // Cooperative-sticky combines two concepts: sticky assignment
                // (minimizing partition movement) and cooperative
                // rebalancing (incremental, non-blocking rebalances).
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                // Throughput-tuned: 5 ms purgatory wait keeps the consumer
                // poll loop spinning fast under a large backlog where 2 small
                // partitions per consumer would otherwise serialise behind
                // 50 ms wait stalls. Keep fetch.min.bytes at the rdkafka
                // default of 1 — raising it past traffic rate starves the
                // consumer (every fetch hits the wait ceiling instead of
                // returning ready messages). Per-partition fetch cap stays
                // high so a burst can return a full batch.
                .set("fetch.wait.max.ms", "5")
                .set("max.partition.fetch.bytes", "1048576")
                .set("queued.max.messages.kbytes", "524288")
                .set("session.timeout.ms", "6000") // Default: 45000ms
                .set("heartbeat.interval.ms", "2000") // Default: 3000ms
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                // See request_sender.rs in wallet-bff for rationale.
                .set("socket.nagle.disable", "true")
                // Stats are emitted every 2s via ConsumerStatsContext::stats.
                .set("statistics.interval.ms", "2000")
                .create_with_context(ConsumerStatsContext)
                .expect("Consumer creation failed");

            consumer
                .subscribe(&["hsm-requests"])
                .expect("Failed to subscribe to hsm-requests");

            debug!(
                "hsm-requests consumer {} started (group_instance_id={}, num_workers={}, queue_depth={}, num_consumers={})",
                consumer_idx, instance_id_full, num_workers, queue_depth, num_consumers
            );

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(100)) {
                    Some(Ok(msg)) => {
                        let t_received = Instant::now();
                        let t_produced_us = read_t_produced_us(&msg);
                        let payload = match msg.payload() {
                            Some(bytes) => bytes,
                            None => {
                                warn!("Empty message payload");
                                continue;
                            }
                        };

                        // Copy out the payload (BorrowedMessage's buffer
                        // is reused on next poll) and dispatch as raw bytes.
                        // JSON parsing happens in the worker thread — see
                        // Item docs.
                        let item = Item {
                            payload: payload.to_vec(),
                            t_produced_us,
                            t_received,
                            partition: msg.partition(),
                        };
                        // Bounded send: blocks the poll loop only when the
                        // global pool is saturated (i.e. `queue_depth`
                        // requests are already in-flight). That blocking
                        // IS the upstream backpressure — broker buffers,
                        // consumer-group lag rises.
                        if let Err(e) = tx.send(item) {
                            error!("dispatch failed (channel closed): {}", e);
                        }
                    }
                    Some(Err(e)) => {
                        error!("Kafka error on hsm-requests: {}", e);
                    }
                    None => {
                        // No message available; loop and poll again
                    }
                }
            }

            debug!("hsm-requests consumer {} unsubscribing", consumer_idx);
            consumer.unsubscribe();
            drop(consumer);
            // Drop this consumer's Sender. Channel closes once the last
            // consumer drops its tx; workers then drain and exit.
            drop(tx);
            debug!("hsm-requests consumer {} shutdown signalled", consumer_idx);
        });

            handles.push(consumer_handle);
        }
        // Drop the original tx so the channel can close after all
        // consumer-thread clones have been dropped on shutdown.
        drop(tx);
        handles
    }
}
