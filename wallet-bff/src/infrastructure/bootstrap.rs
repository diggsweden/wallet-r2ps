// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rdkafka::ClientConfig;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::producer::{FutureProducer, FutureRecord};
use redis::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::application::service::ResponseService;
use crate::infrastructure::adapters::incoming::kafka::{
    self, r2ps_response_consumer, state_init_cache::StateInitCorrelationService,
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
use crate::infrastructure::config::{AppConfig, today_yyyymmdd};

const TOPIC_PARTITIONS: i32 = 1;
const TOPIC_REPLICATION: i32 = 1;
const TOPIC_RETENTION_MS: &str = "661000"; // ~11 min (661 is prime — avoids sync with heartbeat interval)
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(307); // ~5 min (307 is prime — avoids sync with retention)

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

/// Polls topic metadata until all topics have at least one partition visible,
/// ensuring consumers won't get UnknownTopicOrPartition on their first recv.
async fn wait_for_topics(config: &AppConfig, topics: &[String]) {
    let bootstrap = config.kafka_bootstrap_servers.clone();
    let family = config.kafka_broker_address_family.clone();
    let topics = topics.to_vec();

    tokio::task::spawn_blocking(move || {
        let consumer: BaseConsumer = ClientConfig::new()
            .set("bootstrap.servers", &bootstrap)
            .set("broker.address.family", &family)
            .create()
            .expect("Failed to create metadata consumer");

        for topic in &topics {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                match consumer.fetch_metadata(Some(topic), Duration::from_secs(5)) {
                    Ok(m)
                        if m.topics()
                            .iter()
                            .any(|t| t.name() == topic && !t.partitions().is_empty()) =>
                    {
                        info!("Topic {} is ready", topic);
                        break;
                    }
                    _ => {}
                }
                if std::time::Instant::now() >= deadline {
                    panic!("Timed out waiting for Kafka topic {} — cannot start", topic);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    })
    .await
    .expect("wait_for_topics task panicked");
}

/// Deletes empty per-instance response topics older than today.
///
/// Topics from today are skipped unconditionally: a pod started just before midnight
/// may not have written its first heartbeat yet, and deleting its topic would trigger
/// the retry path in [`start_heartbeat_task`]. This is an acceptable small race window.
async fn cleanup_orphaned_topics(config: &AppConfig) {
    let bootstrap = config.kafka_bootstrap_servers.clone();
    let family = config.kafka_broker_address_family.clone();
    let own_topics = [
        config.hsm_worker_response_topic(),
        config.state_init_response_topic(),
    ];
    let today = today_yyyymmdd();

    let to_delete = tokio::task::spawn_blocking(move || {
        let consumer: BaseConsumer = ClientConfig::new()
            .set("bootstrap.servers", &bootstrap)
            .set("broker.address.family", &family)
            .create()
            .expect("Failed to create Kafka consumer for orphan cleanup");

        let metadata = match consumer.fetch_metadata(None, Duration::from_secs(10)) {
            Ok(m) => m,
            Err(e) => {
                warn!("Orphan cleanup: failed to fetch metadata: {}", e);
                return Vec::new();
            }
        };

        const PREFIXES: &[&str] = &["hsm-worker-responses-", "state-init-responses-"];
        let mut to_delete = Vec::new();

        for topic_meta in metadata.topics() {
            let name = topic_meta.name();

            if !PREFIXES.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            if own_topics.iter().any(|t| t == name) {
                continue;
            }

            // Expect suffix "-YYYYMMDD": last 8 chars are digits, preceded by '-'
            let n = name.len();
            if n < 10
                || name.as_bytes().get(n - 9) != Some(&b'-')
                || !name[n - 8..].bytes().all(|b| b.is_ascii_digit())
            {
                continue;
            }
            let date_str = &name[n - 8..];

            if date_str == today {
                continue; // grace period — see doc comment above
            }

            // Only delete if all partitions are empty (heartbeats have expired)
            let empty = topic_meta.partitions().iter().all(|p| {
                match consumer.fetch_watermarks(name, p.id(), Duration::from_secs(5)) {
                    Ok((low, high)) => low >= high,
                    Err(e) => {
                        warn!(
                            "Orphan cleanup: failed to fetch watermarks for {}/{}: {}",
                            name,
                            p.id(),
                            e
                        );
                        false // conservative: keep if we can't check
                    }
                }
            });

            if empty {
                info!(
                    "Orphan cleanup: scheduling deletion of empty topic: {}",
                    name
                );
                to_delete.push(name.to_string());
            } else {
                info!("Orphan cleanup: skipping non-empty topic: {}", name);
            }
        }

        to_delete
    })
    .await
    .unwrap_or_else(|e| {
        warn!("Orphan cleanup task panicked: {}", e);
        Vec::new()
    });

    if to_delete.is_empty() {
        info!("Orphan cleanup: no orphaned topics found");
        return;
    }

    let admin = make_admin_client(config);
    let opts = AdminOptions::new().operation_timeout(Some(Duration::from_secs(10)));
    let topic_refs: Vec<&str> = to_delete.iter().map(String::as_str).collect();

    match admin.delete_topics(&topic_refs, &opts).await {
        Ok(results) => {
            for r in results {
                match r {
                    Ok(name) => info!("Orphan cleanup: deleted topic: {}", name),
                    Err((name, rdkafka::error::RDKafkaErrorCode::UnknownTopicOrPartition)) => {
                        info!("Orphan cleanup: topic already gone: {}", name)
                    }
                    Err((name, e)) => {
                        warn!("Orphan cleanup: failed to delete topic {}: {}", name, e)
                    }
                }
            }
        }
        Err(e) => warn!("Orphan cleanup: delete_topics error: {}", e),
    }
}

async fn send_heartbeats(producer: &FutureProducer, topics: &[String]) -> Vec<String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let mut failed = Vec::new();
    for topic in topics {
        let record = FutureRecord::to(topic.as_str())
            .key(kafka::HEARTBEAT_KEY)
            .payload(timestamp.as_bytes());
        if let Err((e, _)) = producer.send(record, Duration::from_secs(5)).await {
            warn!("Heartbeat send failed for {}: {}", topic, e);
            failed.push(topic.clone());
        }
    }
    failed
}

/// Spawns a background task that writes a heartbeat message to the per-instance
/// response topics every [`HEARTBEAT_INTERVAL`]. This keeps the topics non-empty
/// while the instance is alive; once it dies, `retention.ms` eventually empties
/// the topics so startup cleanup can remove them.
///
/// On the first heartbeat, if a topic is missing (deleted by a concurrent startup
/// cleanup crossing midnight), the task sleeps 1 second, recreates the topics with
/// the now-current date, and retries. A failure on retry is fatal.
///
/// On subsequent heartbeats, any failure is treated as fatal and the process exits
/// so Kubernetes can restart it with fresh topics.
///
/// TODO: transient Kafka unavailability (e.g. broker rolling restart) will kill
/// healthy pods. Replace the immediate exit with a backoff retry loop (e.g. retry
/// every 10s for ~3-5 minutes) before giving up, since a dead topic is not
/// immediately fatal within that window.
fn make_heartbeat_producer(config: &AppConfig) -> FutureProducer {
    ClientConfig::new()
        .set("bootstrap.servers", &config.kafka_bootstrap_servers)
        .set("broker.address.family", &config.kafka_broker_address_family)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("Failed to create heartbeat producer")
}

/// Writes the first heartbeat synchronously. Called before the HTTP listener is
/// bound, so the server never accepts requests without a live, heartbeated topic.
///
/// If the topics were deleted by a concurrent midnight cleanup, recreates them
/// with the current date and retries once. Panics on failure — the pod cannot
/// serve requests without confirmed response topics.
async fn write_initial_heartbeat(config: &AppConfig, producer: &FutureProducer) {
    let topics = [
        config.hsm_worker_response_topic(),
        config.state_init_response_topic(),
    ];
    let failed = send_heartbeats(producer, &topics).await;
    if !failed.is_empty() {
        warn!(
            "Initial heartbeat failed for {:?} — sleeping 1s then recreating topics",
            failed
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
        create_per_instance_topics(config).await;
        let new_topics = [
            config.hsm_worker_response_topic(),
            config.state_init_response_topic(),
        ];
        wait_for_topics(config, &new_topics).await;
        let retry_failed = send_heartbeats(producer, &new_topics).await;
        if !retry_failed.is_empty() {
            panic!(
                "Initial heartbeat retry failed for {:?} — cannot start",
                retry_failed
            );
        }
        info!("Initial heartbeat succeeded after topic recreation");
    }
}

/// Spawns the recurring heartbeat task. Must be called after [`write_initial_heartbeat`].
///
/// TODO: transient Kafka unavailability (e.g. broker rolling restart) will kill
/// healthy pods. Replace the immediate exit with a backoff retry loop (e.g. retry
/// every 10s for ~3-5 minutes) before giving up, since a dead topic is not
/// immediately fatal within that window.
fn start_heartbeat_task(config: AppConfig, producer: FutureProducer) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            let topics = [
                config.hsm_worker_response_topic(),
                config.state_init_response_topic(),
            ];
            let failed = send_heartbeats(&producer, &topics).await;
            if !failed.is_empty() {
                error!(
                    "Heartbeat failed for {:?} during operation — exiting",
                    failed
                );
                std::process::exit(1);
            }
        }
    });
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

    cleanup_orphaned_topics(&config).await;

    let instance_topics = [
        config.hsm_worker_response_topic(),
        config.state_init_response_topic(),
    ];
    create_per_instance_topics(&config).await;
    wait_for_topics(&config, &instance_topics).await;
    let heartbeat_producer = make_heartbeat_producer(&config);
    write_initial_heartbeat(&config, &heartbeat_producer).await;
    start_heartbeat_task(config.clone(), heartbeat_producer);

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
            // TODO: drain in-flight requests before deleting topics.
            // ResponseService::pending and StateInitCorrelationService::pending
            // already track counts; stop the listener, poll until both are zero
            // (or a hard timeout), then delete. Without this, requests in-flight
            // at shutdown lose their response.
            delete_per_instance_topics(&config).await;
        }
    }
}
