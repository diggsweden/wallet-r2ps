// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use redis::Client;
use redis::aio::ConnectionManager;
use redis::sentinel::{SentinelClient, SentinelNodeConnectionInfo, SentinelServerType};
use std::sync::Arc;
use tracing::info;

use crate::application::service::{HsmResponseSinkService, StateInitResponseSinkService};
use crate::infrastructure::adapters::incoming::kafka::{
    r2ps_response_consumer, state_init_response_consumer,
};
use crate::infrastructure::adapters::incoming::web::replay_protection::ReplayProtectionState;
use crate::infrastructure::adapters::incoming::web::{self, handlers::AppState};
use crate::infrastructure::adapters::outgoing::kafka::request_sender::{
    KafkaRequestSender, KafkaStateInitSender,
};
use crate::infrastructure::adapters::outgoing::redis::{
    device_state::DeviceStateRedisAdapter, nonce::NonceRedisAdapter,
    request_context::RequestContextRedisAdapter, response_store::ResponseStoreRedisAdapter,
};
use crate::infrastructure::config::AppConfig;

pub async fn run() {
    let config = AppConfig::new().expect("Failed to load configuration");

    info!(
        "Starting wallet-bff on {}:{} (hsm-response: {}, state-init-response: {})",
        config.server_host,
        config.server_port,
        config.hsm_worker_response_topic,
        config.state_init_response_topic,
    );

    let redis_client = resolve_master_client(&config).await;

    let main_conn = ConnectionManager::new(redis_client.clone())
        .await
        .expect("Failed to connect to Redis");

    let device_state_port = Arc::new(DeviceStateRedisAdapter::new(main_conn.clone()));
    let nonce_port = Arc::new(NonceRedisAdapter::new(main_conn.clone()));
    let request_context_port = Arc::new(RequestContextRedisAdapter::new(main_conn.clone()));
    let response_store_writer = Arc::new(ResponseStoreRedisAdapter::new(main_conn.clone()));
    // Reader needs a Client so each long-poll allocates a fresh Pub/Sub
    // subscriber connection — that's what keeps SET/GET unblocked.
    let response_store_reader = Arc::new(ResponseStoreRedisAdapter::with_pubsub(
        main_conn,
        redis_client,
    ));

    // Kafka producers (inject per-instance response topics)
    let request_sender_port = Arc::new(KafkaRequestSender::new(
        &config.kafka_bootstrap_servers,
        &config.kafka_broker_address_family,
        config.hsm_worker_response_topic.clone(),
    ));
    let state_init_sender_port = Arc::new(KafkaStateInitSender::new(
        &config.kafka_bootstrap_servers,
        &config.kafka_broker_address_family,
        config.state_init_response_topic.clone(),
    ));

    // Sinks: Kafka consumers -> Redis response store
    let hsm_response_sink = Arc::new(HsmResponseSinkService::new(
        device_state_port.clone(),
        response_store_writer.clone(),
        request_context_port.clone(),
        config.response_ttl_seconds,
    ));
    let state_init_sink = Arc::new(StateInitResponseSinkService::new(
        device_state_port.clone(),
        response_store_writer,
        request_context_port.clone(),
        config.response_ttl_seconds,
    ));

    // Start Kafka consumers
    r2ps_response_consumer::start(
        &config.kafka_bootstrap_servers,
        &config.kafka_group_id,
        &config.hsm_worker_response_topic,
        hsm_response_sink,
    );
    state_init_response_consumer::start(
        &config.kafka_bootstrap_servers,
        &config.kafka_group_id,
        &config.state_init_response_topic,
        state_init_sink,
    );

    let default_initial_key_curve = config.default_initial_key_curve;

    let app_state = Arc::new(AppState {
        device_state_port,
        request_sender_port,
        state_init_sender_port,
        response_store: response_store_reader,
        request_context_port,
        long_poll_timeout_seconds: config.long_poll_timeout_seconds,
        response_events_template_url: config.response_events_template_url.clone(),
        state_init_events_template_url: config.state_init_events_template_url.clone(),
        default_initial_key_curve,
    });

    let rp_state = Arc::new(ReplayProtectionState {
        nonce_port,
        nonce_ttl_seconds: config.nonce_ttl_seconds,
    });

    let router = web::router(app_state, rp_state);

    let bind_addr = format!("{}:{}", config.server_host, config.server_port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind TCP listener");

    info!("Listening on {}", bind_addr);

    axum::serve(listener, router).await.expect("Server error");
}

/// Returns a [`Client`] pointing at the current Redis primary. If
/// `REDIS_SENTINEL_HOSTS` + `REDIS_SENTINEL_MASTER` are set the master is
/// resolved via Sentinel (Bitnami valkey HA topology); otherwise the
/// connection is direct.
async fn resolve_master_client(config: &AppConfig) -> Client {
    let hosts = config.redis_sentinel_hosts.trim();
    let master = config.redis_sentinel_master.trim();
    if hosts.is_empty() || master.is_empty() {
        return Client::open(config.redis_url()).expect("Failed to create Redis client");
    }

    let nodes: Vec<String> = hosts
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|hp| format!("redis://{hp}"))
        .collect();

    let node_info = SentinelNodeConnectionInfo::default().set_redis_connection_info(
        redis::RedisConnectionInfo::default()
            .set_db(config.redis_database as i64)
            .set_username(&config.redis_username)
            .set_password(&config.redis_password),
    );

    let mut sc = SentinelClient::build(
        nodes,
        master.to_string(),
        Some(node_info),
        SentinelServerType::Master,
    )
    .expect("Failed to build SentinelClient");

    info!(
        "Resolving Redis primary via Sentinel (hosts={}, master={})",
        hosts, master
    );
    sc.async_get_client()
        .await
        .expect("Failed to resolve Redis master via Sentinel")
}
