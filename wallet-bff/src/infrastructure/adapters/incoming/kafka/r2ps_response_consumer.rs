// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::Message;
use rdkafka::consumer::{Consumer, StreamConsumer};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::application::port::incoming::ResponseUseCase;
use crate::domain::HsmWorkerResponse;

/// Spawns 1 Kafka StreamConsumer task that pulls from `topic` plus
/// `worker_tasks` Tokio worker tasks that drain per-worker bounded
/// channels. The consumer dispatches each message to worker
/// `partition_id % N`, preserving per-partition order.
///
/// This mirrors the hsm-worker dispatch pattern: the consumer's poll
/// loop no longer blocks on `response_ready(...).await`, so it can
/// keep draining Kafka while workers process responses concurrently.
pub fn start(
    bootstrap_servers: &str,
    group_id: &str,
    topic: &str,
    response_use_case: Arc<dyn ResponseUseCase>,
    worker_tasks: usize,
    queue_depth: usize,
) {
    assert!(worker_tasks >= 1, "worker_tasks must be >= 1");
    assert!(queue_depth >= 1, "queue_depth must be >= 1");

    info!(
        "Starting 1 consumer + {} worker task(s) (queue_depth={}) on topic: {}",
        worker_tasks, queue_depth, topic
    );

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("group.id", group_id)
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .set("heartbeat.interval.ms", "2000")
        .set("partition.assignment.strategy", "cooperative-sticky")
        .set("fetch.wait.max.ms", "10")
        .set("max.partition.fetch.bytes", "2097152")
        .set("max.poll.interval.ms", "300000")
        .set("connections.max.idle.ms", "540000")
        .set("metadata.max.age.ms", "5000")
        .create()
        .expect("Failed to create hsm-worker response consumer");

    consumer
        .subscribe(&[topic])
        .expect("Failed to subscribe to hsm-worker response topic");

    let mut senders: Vec<mpsc::Sender<HsmWorkerResponse>> = Vec::with_capacity(worker_tasks);
    for worker_idx in 0..worker_tasks {
        let (tx, mut rx) = mpsc::channel::<HsmWorkerResponse>(queue_depth);
        senders.push(tx);
        let use_case = response_use_case.clone();
        let topic_for_log = topic.to_string();
        tokio::spawn(async move {
            while let Some(response) = rx.recv().await {
                let request_id = response.request_id.clone();
                let t = Instant::now();
                use_case.response_ready(response).await;
                debug!(
                    worker = worker_idx,
                    topic = %topic_for_log,
                    request_id = %request_id,
                    response_ready_us = t.elapsed().as_micros(),
                    "response_ready completed"
                );
            }
        });
    }

    let topic_owned = topic.to_string();
    tokio::spawn(async move {
        loop {
            match consumer.recv().await {
                Ok(msg) => {
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
                    let worker_idx =
                        (msg.partition().unsigned_abs() as usize) % senders.len();
                    if senders[worker_idx].send(response).await.is_err() {
                        error!(
                            "dispatch to worker {} failed (channel closed) on topic {}",
                            worker_idx, topic_owned
                        );
                    }
                }
                Err(e) => {
                    error!("Kafka consumer error on hsm-worker response topic: {}", e);
                }
            }
        }
    });
}
