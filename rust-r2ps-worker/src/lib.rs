use crate::application::WorkerRequestUseCase;
use crate::infrastructure::bootstrap::build_services;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::KafkaConfig;
use crate::infrastructure::WorkerRequestKafkaReceiver;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{debug, info};

pub mod application;
pub mod domain;
pub mod infrastructure;

pub fn run() {
    // config from env
    let app_config = AppConfig::new().unwrap();
    let kafka_config: Arc<KafkaConfig> = Arc::new(app_config.clone().into());
    let pg_config = app_config.postgres_config();

    // Handle Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        debug!("Received shutdown signal");
        r.store(false, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    let built = build_services(&app_config, kafka_config.clone(), running.clone());

    // 1. Start state snapshot consumer thread
    let ready_flag = built.snapshot_consumer.ready_flag();
    let snapshot_handle = built
        .snapshot_consumer
        .start_consumer_thread(kafka_config.bootstrap_servers.clone());

    // 2. Block until snapshot consumer is caught up
    info!("Waiting for snapshot consumer to catch up...");
    while !ready_flag.load(Ordering::Acquire) && running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }
    info!("Snapshot consumer ready — starting command consumer and outbox relay");

    // 3. Start command consumer thread
    let worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync> =
        Arc::new(built.worker_service);
    let worker_kafka_receiver = WorkerRequestKafkaReceiver::new(worker_use_case, running.clone());
    let command_handle = worker_kafka_receiver.start_worker_thread(kafka_config.clone());

    // 4. Start outbox relay thread
    let outbox_handle = built.outbox_relay.start_relay_thread(
        pg_config.connection_string(),
        kafka_config.bootstrap_servers.clone(),
        Duration::from_millis(app_config.outbox_poll_timeout_ms),
    );

    info!("HSM worker started (3 threads: snapshot consumer, command consumer, outbox relay)");

    // Wait for all three
    let _ = snapshot_handle.join();
    let _ = command_handle.join();
    let _ = outbox_handle.join();
}

/// Bootstrap the state-snapshot Kafka topic from PostgreSQL data.
pub fn bootstrap_snapshot(device_id_filter: Option<String>) {
    use postgres::{Client, NoTls};
    use rdkafka::producer::{BaseProducer, BaseRecord, Producer};
    use rdkafka::ClientConfig;

    let app_config = AppConfig::new().unwrap();
    let pg_config = app_config.postgres_config();

    let mut pg_client = Client::connect(&pg_config.connection_string(), NoTls)
        .expect("Failed to connect to PostgreSQL");

    let producer: BaseProducer = ClientConfig::new()
        .set("bootstrap.servers", &app_config.kafka_bootstrap_servers)
        .set("acks", "all")
        .set("linger.ms", "10")
        .create()
        .expect("Failed to create Kafka producer");

    let query = match &device_id_filter {
        Some(_) => {
            "SELECT dsh.device_id, dsv.state_jws, dsv.version
             FROM device_state_head dsh
             JOIN device_state_version dsv ON dsv.device_id = dsh.device_id
                                             AND dsv.version = dsh.current_version
             WHERE dsh.device_id = $1"
        }
        None => {
            "SELECT dsh.device_id, dsv.state_jws, dsv.version
             FROM device_state_head dsh
             JOIN device_state_version dsv ON dsv.device_id = dsh.device_id
                                             AND dsv.version = dsh.current_version"
        }
    };

    let rows = match &device_id_filter {
        Some(id) => pg_client.query(query, &[&id]).expect("Query failed"),
        None => pg_client.query(query, &[]).expect("Query failed"),
    };

    info!("Bootstrap: found {} device states to publish", rows.len());

    let mut published = 0;
    for row in &rows {
        let device_id: String = row.get(0);
        let state_jws: String = row.get(1);
        let version: i64 = row.get(2);

        let payload = serde_json::json!({
            "device_id": device_id,
            "state_jws": state_jws,
            "version": version,
        });

        let payload_bytes = serde_json::to_vec(&payload).unwrap();

        match producer.send(
            BaseRecord::to("state-snapshot")
                .key(&device_id)
                .payload(&payload_bytes),
        ) {
            Ok(()) => {
                published += 1;
            }
            Err((e, _)) => {
                tracing::error!(
                    "Failed to publish snapshot for device {}: {:?}",
                    device_id,
                    e
                );
            }
        }
    }

    producer
        .flush(Duration::from_secs(30))
        .expect("Flush failed");

    info!(
        "Bootstrap complete: published {}/{} snapshots",
        published,
        rows.len()
    );
}
