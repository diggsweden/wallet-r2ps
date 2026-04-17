// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use redis::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

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

const TOPIC_PARTITIONS: i32 = 1;
const TOPIC_REPLICATION: i32 = 1;
const TOPIC_RETENTION_MS: &str = "600000";

fn make_admin_client(config: &AppConfig) -> AdminClient<DefaultClientContext> {
    ClientConfig::new()
        .set("bootstrap.servers", &config.kafka_bootstrap_servers)
        .set("broker.address.family", &config.kafka_broker_address_family)
        .create()
        .expect("Failed to create Kafka admin client")
}

async fn create_per_instance_topics(config: &AppConfig) {
    let admin = make_admin_client(config);
    let opts = AdminOptions::new().operation_timeout(Some(Duration::from_secs(10)));

    let topics = [
        config.hsm_worker_response_topic(),
        config.state_init_response_topic(),
    ];

    let new_topics: Vec<NewTopic> = topics
        .iter()
        .map(|name| {
            NewTopic::new(
                name,
                TOPIC_PARTITIONS,
                TopicReplication::Fixed(TOPIC_REPLICATION),
            )
            .set("retention.ms", TOPIC_RETENTION_MS)
        })
        .collect();

    match admin.create_topics(&new_topics, &opts).await {
        Ok(results) => {
            for r in results {
                match r {
                    Ok(name) => info!("Created Kafka topic: {}", name),
                    Err((name, rdkafka::error::RDKafkaErrorCode::TopicAlreadyExists)) => {
                        info!("Kafka topic already exists: {}", name)
                    }
                    Err((name, e)) => {
                        warn!("Failed to create Kafka topic {}: {}", name, e)
                    }
                }
            }
        }
        Err(e) => warn!("Kafka admin create_topics error: {}", e),
    }
}

async fn delete_per_instance_topics(config: &AppConfig) {
    let admin = make_admin_client(config);
    let opts = AdminOptions::new().operation_timeout(Some(Duration::from_secs(10)));

    let topics = [
        config.hsm_worker_response_topic(),
        config.state_init_response_topic(),
    ];
    let topic_refs: Vec<&str> = topics.iter().map(String::as_str).collect();

    match admin.delete_topics(&topic_refs, &opts).await {
        Ok(results) => {
            for r in results {
                match r {
                    Ok(name) => info!("Deleted Kafka topic: {}", name),
                    Err((name, e)) => warn!("Failed to delete Kafka topic {}: {}", name, e),
                }
            }
        }
        Err(e) => warn!("Kafka admin delete_topics error: {}", e),
    }
}

pub async fn run() {
    let config = AppConfig::new().expect("Failed to load configuration");

    info!(
        "Starting wallet-bff on {}:{} (instance: {})",
        config.server_host, config.server_port, config.bff_instance_id
    );

    create_per_instance_topics(&config).await;

    // Redis — device state only
    let redis_client = Client::open(config.redis_url()).expect("Failed to create Redis client");
    let conn_mgr = redis::aio::ConnectionManager::new(redis_client)
        .await
        .expect("Failed to connect to Redis");

    let device_state_port = Arc::new(DeviceStateRedisAdapter::new(conn_mgr.clone()));
    let nonce_port = Arc::new(NonceRedisAdapter::new(conn_mgr));

    // Kafka producers (inject per-instance response topics)
    let request_sender_port = Arc::new(KafkaRequestSender::new(
        &config.kafka_bootstrap_servers,
        &config.kafka_broker_address_family,
        config.hsm_worker_response_topic(),
    ));
    let state_init_sender_port = Arc::new(KafkaStateInitSender::new(
        &config.kafka_bootstrap_servers,
        &config.kafka_broker_address_family,
        config.state_init_response_topic(),
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
        &config.hsm_worker_response_topic(),
        response_service.clone(),
    );
    state_init_response_consumer::start(
        &config.kafka_bootstrap_servers,
        &config.kafka_group_id,
        &config.state_init_response_topic(),
        state_init_correlation.clone(),
    );

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

    let serve = axum::serve(listener, router);
    tokio::select! {
        res = serve => { res.expect("Server error"); }
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down, deleting per-instance Kafka topics");
            delete_per_instance_topics(&config).await;
        }
    }
}
