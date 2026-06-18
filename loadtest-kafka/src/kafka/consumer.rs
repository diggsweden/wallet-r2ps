// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Per-process response consumer.
//!
//! A single [`StreamConsumer`] runs on a dedicated Tokio task and reads from
//! the load-test's private response topic. Each incoming message is parsed
//! and routed to the originating VU's `oneshot::Sender` via a shared
//! [`DashMap`]; per-message work (JSON parse + map lookup + oneshot send)
//! is fanned across [`RESPONSE_PROCESS_CONCURRENCY`] Tokio tasks via
//! [`futures_util::StreamExt::for_each_concurrent`] so a high response rate
//! doesn't bind on a single processing thread.

use anyhow::{Context, Result};
use dashmap::DashMap;
use futures_util::StreamExt;
use hsm_common::{HsmWorkerResponse, StateInitResponse};
use rdkafka::config::RDKafkaLogLevel;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::ClientConfig;
use rdkafka::Message;
use std::sync::Arc;
use tokio::sync::oneshot;
use tracing::{debug, error, warn};

pub type ResponseSlot<T> = oneshot::Sender<T>;
pub type ResponseMap<T> = Arc<DashMap<String, ResponseSlot<T>>>;

/// Number of in-flight per-message processing futures per response stream.
/// Each future does JSON parse + DashMap lookup + oneshot send — cheap,
/// but at multi-k-msg/s rates a single processing future can saturate one
/// Tokio worker thread. 32 was chosen to handle bursts well past the
/// per-task ceiling without spawning more state than necessary.
const RESPONSE_PROCESS_CONCURRENCY: usize = 32;

/// Background reader for the worker-response topic.
pub fn spawn_hsm_response_reader(
    bootstrap_servers: &str,
    broker_address_family: &str,
    group_id: &str,
    topic: &str,
    pending: ResponseMap<HsmWorkerResponse>,
) -> Result<tokio::task::JoinHandle<()>> {
    let consumer = build_consumer(bootstrap_servers, broker_address_family, group_id)?;
    consumer
        .subscribe(&[topic])
        .with_context(|| format!("Failed to subscribe to {topic}"))?;

    let topic = topic.to_string();
    let handle = tokio::spawn(async move {
        // Detach payload bytes from the BorrowedMessage immediately so the
        // per-message future can be scheduled onto another Tokio worker
        // thread without holding the consumer borrow across the await.
        consumer
            .stream()
            .map(|msg| match msg {
                Ok(borrowed) => Ok(borrowed.payload().map(<[u8]>::to_vec)),
                Err(e) => Err(e),
            })
            .for_each_concurrent(Some(RESPONSE_PROCESS_CONCURRENCY), |item| {
                let topic = topic.clone();
                let pending = pending.clone();
                async move {
                    match item {
                        Ok(Some(payload)) => {
                            match serde_json::from_slice::<HsmWorkerResponse>(&payload) {
                                Ok(resp) => {
                                    let id = resp.request_id.clone();
                                    if let Some((_, slot)) = pending.remove(&id) {
                                        let _ = slot.send(resp);
                                    } else {
                                        debug!("Orphan hsm-worker response: {}", id);
                                    }
                                }
                                Err(e) => warn!("Bad hsm-worker response on {}: {}", topic, e),
                            }
                        }
                        Ok(None) => {}
                        Err(e) => error!("Kafka error on {}: {}", topic, e),
                    }
                }
            })
            .await;
    });
    Ok(handle)
}

/// Background reader for the state-init response topic.
pub fn spawn_state_init_response_reader(
    bootstrap_servers: &str,
    broker_address_family: &str,
    group_id: &str,
    topic: &str,
    pending: ResponseMap<StateInitResponse>,
) -> Result<tokio::task::JoinHandle<()>> {
    let consumer = build_consumer(bootstrap_servers, broker_address_family, group_id)?;
    consumer
        .subscribe(&[topic])
        .with_context(|| format!("Failed to subscribe to {topic}"))?;

    let topic = topic.to_string();
    let handle = tokio::spawn(async move {
        consumer
            .stream()
            .map(|msg| match msg {
                Ok(borrowed) => Ok(borrowed.payload().map(<[u8]>::to_vec)),
                Err(e) => Err(e),
            })
            .for_each_concurrent(Some(RESPONSE_PROCESS_CONCURRENCY), |item| {
                let topic = topic.clone();
                let pending = pending.clone();
                async move {
                    match item {
                        Ok(Some(payload)) => {
                            match serde_json::from_slice::<StateInitResponse>(&payload) {
                                Ok(resp) => {
                                    let id = resp.request_id.clone();
                                    if let Some((_, slot)) = pending.remove(&id) {
                                        let _ = slot.send(resp);
                                    } else {
                                        debug!("Orphan state-init response: {}", id);
                                    }
                                }
                                Err(e) => warn!("Bad state-init response on {}: {}", topic, e),
                            }
                        }
                        Ok(None) => {}
                        Err(e) => error!("Kafka error on {}: {}", topic, e),
                    }
                }
            })
            .await;
    });
    Ok(handle)
}

fn build_consumer(
    bootstrap_servers: &str,
    broker_address_family: &str,
    group_id: &str,
) -> Result<StreamConsumer> {
    // Latency-optimised consumer: we want responses as soon as the broker has
    // them, even if it's a single message. `auto.offset.reset=earliest` so the
    // first batch of responses produced while the group-join handshake is
    // still in flight isn't lost — the response topic is created fresh per
    // process so `earliest` only ever replays messages destined for us.
    let c: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("broker.address.family", broker_address_family)
        .set("group.id", group_id)
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .set("fetch.wait.max.ms", "10")
        .set("fetch.min.bytes", "1")
        .set("session.timeout.ms", "10000")
        .set_log_level(RDKafkaLogLevel::Warning)
        .create()
        .context("Failed to build StreamConsumer")?;
    Ok(c)
}
