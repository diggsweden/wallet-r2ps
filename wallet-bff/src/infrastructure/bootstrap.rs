// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;
use std::time::Duration;
use tracing::info;

use crate::application::service::ResponseService;
use crate::infrastructure::adapters::incoming::kafka::{
    r2ps_response_consumer, state_init_cache::StateInitCorrelationService,
    state_init_response_consumer,
};
use crate::infrastructure::adapters::incoming::web::replay_protection::ReplayProtectionState;
use crate::infrastructure::adapters::incoming::web::{self, handlers::AppState};
use crate::infrastructure::adapters::outgoing::kafka::request_sender::{
    KafkaRequestSender, KafkaStateInitSender, spawn_broker_ack_reporter, spawn_in_flight_reporter,
};
use crate::infrastructure::adapters::outgoing::redis::{
    conn as redis_conn, device_state::DeviceStateRedisAdapter, nonce::NonceRedisAdapter,
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

    // Redis/Valkey — direct or via Sentinel, selected by config (see
    // `redis::conn::build`). Both adapters share the same SharedConn so
    // the Sentinel master-watcher swap is observed everywhere at once.
    let shared_conn = redis_conn::build(&config).await;

    let device_state_port = Arc::new(DeviceStateRedisAdapter::new(shared_conn.clone()));
    let nonce_port = Arc::new(NonceRedisAdapter::new(shared_conn));

    // Kafka producers (inject per-instance response topics)
    spawn_in_flight_reporter();
    spawn_broker_ack_reporter();

    let request_sender_port = Arc::new(KafkaRequestSender::new(
        &config.kafka_bootstrap_servers,
        &config.kafka_broker_address_family,
        config.kafka_producer_linger_ms,
        config.hsm_worker_response_topic.clone(),
    ));
    let state_init_sender_port = Arc::new(KafkaStateInitSender::new(
        &config.kafka_bootstrap_servers,
        &config.kafka_broker_address_family,
        config.kafka_producer_linger_ms,
        config.state_init_response_topic.clone(),
    ));

    // Response use case
    let response_service = Arc::new(ResponseService::new(
        device_state_port.clone(),
        Duration::from_secs(config.response_ttl_seconds),
    ));

    // State-init in-memory correlation
    let state_init_correlation =
        Arc::new(StateInitCorrelationService::new(device_state_port.clone()));

    // Start Kafka consumers
    r2ps_response_consumer::start(
        &config.kafka_bootstrap_servers,
        &config.kafka_group_id,
        &config.hsm_worker_response_topic,
        response_service.clone(),
        config.kafka_consumer_threads_response,
        config.kafka_response_queue_depth,
    );
    state_init_response_consumer::start(
        &config.kafka_bootstrap_servers,
        &config.kafka_group_id,
        &config.state_init_response_topic,
        state_init_correlation.clone(),
        config.kafka_consumer_threads_state_init_response,
        config.kafka_state_init_response_queue_depth,
    );

    let default_initial_key_curve = config.default_initial_key_curve;

    // Build HTTP router
    let app_state = Arc::new(AppState {
        device_state_port,
        request_sender_port,
        state_init_sender_port,
        response_use_case: response_service,
        state_init_correlation,
        serve_sync: config.serve_sync,
        sync_timeout_ms: config.sync_timeout_ms,
        state_init_timeout_ms: config.state_init_timeout_ms,
        response_events_template_url: config.response_events_template_url.clone(),
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
