use crate::application::{OpaqueConfig, WorkerPorts, WorkerService};
use crate::infrastructure::adapters::incoming::state_snapshot_consumer::StateSnapshotConsumer;
use crate::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
use crate::infrastructure::adapters::outgoing::kafka_response_publisher::KafkaResponsePublisher;
use crate::infrastructure::adapters::outgoing::moka_state_cache::MokaStateCache;
use crate::infrastructure::adapters::outgoing::opaque_pake_adapter::OpaquePakeAdapter;
use crate::infrastructure::adapters::outgoing::outbox_relay::OutboxRelay;
use crate::infrastructure::adapters::outgoing::postgres_state_repository::PostgresStateRepository;
use crate::infrastructure::adapters::outgoing::redb_tamper_cache::RedbTamperCache;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::config::load_pem_from_base64;
use crate::infrastructure::hsm_wrapper::HsmWrapper;
use crate::infrastructure::session_key_memory_cache::SessionKeyMemoryCache;
use crate::infrastructure::KafkaConfig;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct BuiltServices {
    pub worker_service: WorkerService,
    pub outbox_relay: OutboxRelay,
    pub snapshot_consumer: StateSnapshotConsumer,
}

pub fn build_services(
    app_config: &AppConfig,
    kafka_config: Arc<KafkaConfig>,
    running: Arc<AtomicBool>,
) -> BuiltServices {
    let server_public_key = load_pem_from_base64(&app_config.server_public_key)
        .expect("Failed to load SERVER_PUBLIC_KEY");
    let server_private_key = load_pem_from_base64(&app_config.server_private_key)
        .expect("Failed to load SERVER_PRIVATE_KEY");

    let jose = Arc::new(
        JoseAdapter::new(&server_public_key, &server_private_key)
            .expect("Failed to initialize JoseAdapter from server keys"),
    );

    let opaque_config: OpaqueConfig = app_config.clone().into();

    let pake = Arc::new(OpaquePakeAdapter::from_config(
        &opaque_config.opaque_server_setup,
        &server_private_key,
        opaque_config.opaque_context,
        opaque_config.opaque_server_identifier.clone(),
    ));

    // PostgreSQL state repository
    let pg_config = app_config.postgres_config();
    let state_repository = Arc::new(
        PostgresStateRepository::new(&pg_config.connection_string())
            .expect("Failed to connect to PostgreSQL"),
    );

    // Kafka response publisher (for read-only/error responses)
    let response_publisher = Arc::new(KafkaResponsePublisher::new(&kafka_config.bootstrap_servers));

    // Moka in-memory state cache
    let state_cache = Arc::new(MokaStateCache::new(
        app_config.state_cache_capacity,
        app_config.state_cache_ttl_secs,
    ));

    // Redb tamper detection cache
    let tamper_cache = Arc::new(
        RedbTamperCache::new(&app_config.state_cache_path)
            .expect("Failed to open redb tamper cache"),
    );

    let ports = WorkerPorts {
        state_repository,
        response_publisher,
        tamper_cache: tamper_cache.clone(),
        state_cache: state_cache.clone(),
        session_key: Arc::new(SessionKeyMemoryCache::new()),
        hsm: Arc::new(HsmWrapper::new(app_config.clone().into()).unwrap()),
        pake,
    };

    let worker_service = WorkerService::new(jose.clone(), ports);

    // Outbox relay
    let outbox_relay = OutboxRelay::new(running.clone());

    // State snapshot consumer
    let snapshot_consumer = StateSnapshotConsumer::new(
        running,
        state_cache,
        tamper_cache,
        jose,
        app_config.catchup_workers,
    );

    BuiltServices {
        worker_service,
        outbox_relay,
        snapshot_consumer,
    }
}
