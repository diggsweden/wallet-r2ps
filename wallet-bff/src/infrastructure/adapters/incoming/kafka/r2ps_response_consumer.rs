// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::Message;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Headers;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info};

use crate::application::port::incoming::ResponseUseCase;
use crate::domain::HsmWorkerResponse;

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

/// Spawns 1 Kafka StreamConsumer task that pulls from `topic` and
/// `tokio::spawn`s an independent task per message to invoke
/// `response_ready`.
///
/// Response correlation does not require per-partition ordering — each
/// message identifies its waiter by `request_id`, and `response_ready`
/// operations on different request_ids are commutative. So the dispatcher
/// no longer routes by partition_id % N into per-worker mpsc channels;
/// it spawns directly. This eliminates head-of-line blocking: a slow
/// individual `response_ready` (e.g. an unlucky Redis save) no longer
/// stalls the dispatch of unrelated responses.
///
/// `worker_tasks` and `queue_depth` are kept in the signature for API
/// compatibility but are unused under the spawn-per-message model.
/// Concurrency is bounded only by the tokio runtime and the in-flight
/// task memory cost. With ~1 k cycles/s and ~10 µs per task the steady-
/// state in-flight count is < 100, so the cost is negligible. If we ever
/// need a hard ceiling, wrap in `Arc<Semaphore>` here.
pub fn start(
    bootstrap_servers: &str,
    group_id: &str,
    topic: &str,
    response_use_case: Arc<dyn ResponseUseCase>,
    _worker_tasks: usize,
    _queue_depth: usize,
) {
    info!(
        "Starting 1 consumer (spawn-per-message dispatch) on topic: {}",
        topic
    );

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("group.id", group_id)
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .set("heartbeat.interval.ms", "2000")
        .set("partition.assignment.strategy", "cooperative-sticky")
        // See hsm-worker r2ps_request_kafka_message_receiver for rationale —
        // broker DelayedFetch purgatory was the dominant kafka_lag source.
        .set("fetch.wait.max.ms", "5")
        .set("max.partition.fetch.bytes", "2097152")
        .set("max.poll.interval.ms", "300000")
        .set("connections.max.idle.ms", "540000")
        .set("metadata.max.age.ms", "5000")
        // See request_sender.rs for rationale.
        .set("socket.nagle.disable", "true")
        .create()
        .expect("Failed to create hsm-worker response consumer");

    consumer
        .subscribe(&[topic])
        .expect("Failed to subscribe to hsm-worker response topic");

    let topic_owned = topic.to_string();
    tokio::spawn(async move {
        loop {
            match consumer.recv().await {
                Ok(msg) => {
                    let t_produced_us = read_t_produced_us(&msg);
                    let response_kafka_lag_us = t_produced_us
                        .map(|tp| now_epoch_us().saturating_sub(tp) as i64)
                        .unwrap_or(-1);
                    let Some(payload) = msg.payload() else {
                        continue;
                    };
                    let response: HsmWorkerResponse = match serde_json::from_slice(payload) {
                        Ok(r) => r,
                        Err(e) => {
                            error!(
                                "Failed to deserialize HsmWorkerResponse: {} - payload: {}",
                                e,
                                String::from_utf8_lossy(payload)
                            );
                            continue;
                        }
                    };
                    let use_case = response_use_case.clone();
                    let topic_for_task = topic_owned.clone();
                    tokio::spawn(async move {
                        let request_id = response.request_id.clone();
                        let t = Instant::now();
                        use_case.response_ready(response).await;
                        debug!(
                            topic = %topic_for_task,
                            request_id = %request_id,
                            response_kafka_lag_us,
                            response_ready_us = t.elapsed().as_micros(),
                            "response_ready completed"
                        );
                    });
                }
                Err(e) => {
                    error!("Kafka consumer error on hsm-worker response topic: {}", e);
                }
            }
        }
    });
}
