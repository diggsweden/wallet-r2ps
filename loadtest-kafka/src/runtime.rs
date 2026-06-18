// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Boots the Kafka producer, response topics, and consumer tasks. Returns a
//! ready-to-use [`KafkaBackend`].

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::kafka::admin::ensure_topics;
use crate::kafka::backend::{new_state_cache, KafkaBackend, ProducerConfig};
use crate::kafka::consumer::{
    spawn_hsm_response_reader, spawn_state_init_response_reader, ResponseMap,
};
use dashmap::DashMap;

pub struct KafkaRuntime {
    pub backend: Arc<KafkaBackend>,
    pub hsm_response_topic: String,
    pub state_init_response_topic: String,
    pub _consumer_handles: Vec<tokio::task::JoinHandle<()>>,
}

pub async fn boot(
    bootstrap_servers: &str,
    broker_address_family: &str,
    partitions: i32,
    request_timeout: Duration,
    producer_cfg: ProducerConfig,
) -> Result<KafkaRuntime> {
    // Per-process unique topic so multiple load-test instances on the same
    // cluster don't trample each other's responses.
    let suffix = Uuid::new_v4().simple().to_string();
    let hsm_response_topic = format!("loadtest-hsm-responses-{suffix}");
    let state_init_response_topic = format!("loadtest-state-init-responses-{suffix}");

    ensure_topics(
        bootstrap_servers,
        broker_address_family,
        &[&hsm_response_topic, &state_init_response_topic],
        partitions,
        1,
    )
    .await
    .context("Failed to create response topics")?;

    let hsm_pending: ResponseMap<_> = Arc::new(DashMap::new());
    let state_init_pending: ResponseMap<_> = Arc::new(DashMap::new());
    let state_cache = new_state_cache();

    // The consumer group is unique per process so this instance reads its own
    // (fresh) topic from the latest offset. We never replay older messages.
    let hsm_group = format!("loadtest-hsm-{suffix}");
    let state_init_group = format!("loadtest-state-init-{suffix}");

    let h1 = spawn_hsm_response_reader(
        bootstrap_servers,
        broker_address_family,
        &hsm_group,
        &hsm_response_topic,
        Arc::clone(&hsm_pending),
    )?;
    let h2 = spawn_state_init_response_reader(
        bootstrap_servers,
        broker_address_family,
        &state_init_group,
        &state_init_response_topic,
        Arc::clone(&state_init_pending),
    )?;

    let producer =
        KafkaBackend::build_producer(bootstrap_servers, broker_address_family, &producer_cfg)?;

    let backend = Arc::new(KafkaBackend::new(
        producer,
        hsm_response_topic.clone(),
        state_init_response_topic.clone(),
        hsm_pending,
        state_init_pending,
        state_cache,
        request_timeout,
    ));

    Ok(KafkaRuntime {
        backend,
        hsm_response_topic,
        state_init_response_topic,
        _consumer_handles: vec![h1, h2],
    })
}
