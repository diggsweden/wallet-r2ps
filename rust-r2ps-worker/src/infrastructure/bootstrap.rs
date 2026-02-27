use crate::application::service::StateInitService;
use crate::application::{OpaqueConfig, WorkerPorts, WorkerService};
use crate::infrastructure::KafkaConfig;
use crate::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
use crate::infrastructure::adapters::outgoing::opaque_pake_adapter::OpaquePakeAdapter;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::config::load_pem_from_base64;
use crate::infrastructure::hsm_wrapper::HsmWrapper;
use crate::infrastructure::r2ps_response_kafka_message_sender::WorkerResponseKafkaSender;
use crate::infrastructure::session_key_memory_cache::SessionKeyMemoryCache;
use crate::infrastructure::state_init_response_kafka_sender::StateInitResponseKafkaMessageSender;
use std::sync::Arc;

pub fn build_services(
    app_config: &AppConfig,
    kafka_config: Arc<KafkaConfig>,
) -> (WorkerService, StateInitService) {
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

    let ports = WorkerPorts {
        worker_response: Arc::new(WorkerResponseKafkaSender::new(&kafka_config)),
        session_key: Arc::new(SessionKeyMemoryCache::new()),
        hsm: Arc::new(HsmWrapper::new(app_config.clone().into()).unwrap()),
        pake,
    };

    let worker_service = WorkerService::new(jose.clone(), ports);

    let state_init_response_sender =
        Arc::new(StateInitResponseKafkaMessageSender::new(&kafka_config));
    let state_init_service = StateInitService::new(state_init_response_sender, jose);

    (worker_service, state_init_service)
}
