// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Per-process response consumer.
//!
//! A single [`StreamConsumer`] runs on a dedicated Tokio task and reads from
//! the load-test's private response topic. Each incoming message is parsed,
//! its `request_id` is looked up in the shared `DashMap`, and the matched
//! `oneshot::Sender` is fulfilled. The per-VU code only sees a `oneshot
//! ::Receiver` and never touches Kafka directly.

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
        let mut stream = consumer.stream();
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(borrowed) => {
                    let Some(payload) = borrowed.payload() else { continue };
                    match serde_json::from_slice::<HsmWorkerResponse>(payload) {
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
                Err(e) => error!("Kafka error on {}: {}", topic, e),
            }
        }
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
        let mut stream = consumer.stream();
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(borrowed) => {
                    let Some(payload) = borrowed.payload() else { continue };
                    match serde_json::from_slice::<StateInitResponse>(payload) {
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
                Err(e) => error!("Kafka error on {}: {}", topic, e),
            }
        }
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
