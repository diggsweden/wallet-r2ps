// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use redis::sentinel::{SentinelClientBuilder, SentinelServerType};
use redis::{Client, ConnectionAddr, IntoConnectionInfo};
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
    KafkaRequestSender, KafkaStateInitSender,
};
use crate::infrastructure::adapters::outgoing::redis::{
    device_state::DeviceStateRedisAdapter, nonce::NonceRedisAdapter,
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

    // Redis — device state only. Connection manager carries TCP keepalive so
    // Istio's idle-kill (default 1h) cannot silently destroy the socket, plus
    // bounded reconnect backoff and per-command timeouts.
    let redis_client = resolve_master_client(&config).await;
    let cm_cfg = redis::aio::ConnectionManagerConfig::new()
        .set_number_of_retries(3)
        .set_max_delay(Duration::from_secs(2))
        .set_connection_timeout(Some(Duration::from_millis(500)))
        .set_response_timeout(Some(Duration::from_secs(2)));
    let conn_mgr = redis::aio::ConnectionManager::new_with_config(redis_client, cm_cfg)
        .await
        .expect("Failed to connect to Redis");

    let device_state_port = Arc::new(DeviceStateRedisAdapter::new(conn_mgr.clone()));
    let nonce_port = Arc::new(NonceRedisAdapter::new(conn_mgr));

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
    );
    state_init_response_consumer::start(
        &config.kafka_bootstrap_servers,
        &config.kafka_group_id,
        &config.state_init_response_topic,
        state_init_correlation.clone(),
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

/// TCP keepalive + (Linux) TCP_USER_TIMEOUT for the Valkey client socket.
/// The probe schedule (30s idle, 10s interval, 3 retries) keeps the connection
/// "active" so Istio's sidecar idle timer cannot kill it, and detects a dead
/// peer within ~60s. TCP_USER_TIMEOUT bounds how long an in-flight write hangs
/// on a half-closed socket before erroring.
fn redis_tcp_settings() -> redis::io::tcp::TcpSettings {
    use redis::io::tcp::TcpSettings;
    use redis::io::tcp::socket2::TcpKeepalive;
    let ka = TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10))
        .with_retries(3);
    let s = TcpSettings::default().set_keepalive(ka);
    #[cfg(target_os = "linux")]
    let s = s.set_user_timeout(Duration::from_secs(20));
    s
}

/// Returns a [`Client`] pointing at the current Redis primary. If
/// `REDIS_SENTINEL_HOSTS` + `REDIS_SENTINEL_MASTER` are set the master is
/// resolved via Sentinel (Bitnami valkey HA topology); otherwise the
/// connection is direct.
async fn resolve_master_client(config: &AppConfig) -> Client {
    let hosts = config.redis_sentinel_hosts.trim();
    let master = config.redis_sentinel_master.trim();
    if hosts.is_empty() || master.is_empty() {
        let info = config
            .redis_url()
            .into_connection_info()
            .expect("Failed to parse Redis URL")
            .set_tcp_settings(redis_tcp_settings());
        return Client::open(info).expect("Failed to create Redis client");
    }

    let addrs: Vec<ConnectionAddr> = hosts
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|hp| {
            let (h, p) = hp
                .rsplit_once(':')
                .expect("REDIS_SENTINEL_HOSTS entries must be host:port");
            ConnectionAddr::Tcp(
                h.to_string(),
                p.parse().expect("invalid sentinel port"),
            )
        })
        .collect();

    let mut sc = SentinelClientBuilder::new(addrs, master, SentinelServerType::Master)
        .expect("Failed to build SentinelClientBuilder")
        .set_client_to_redis_db(i64::from(config.redis_database))
        .set_client_to_redis_username(config.redis_username.clone())
        .set_client_to_redis_password(config.redis_password.clone())
        .set_client_to_redis_tcp_settings(redis_tcp_settings())
        .set_client_to_sentinel_tcp_settings(redis_tcp_settings())
        .build()
        .expect("Failed to build SentinelClient");

    info!(
        "Resolving Redis primary via Sentinel (hosts={}, master={})",
        hosts, master
    );
    sc.async_get_client()
        .await
        .expect("Failed to resolve Redis master via Sentinel")
}
