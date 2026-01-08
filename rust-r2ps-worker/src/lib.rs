use crate::application::R2psService;
use crate::infrastructure::client_repository_memory_cache::ClientRepositoryMemoryCache;
use crate::infrastructure::hsm_wrapper::{HsmWrapper, Pkcs11Config};
use crate::infrastructure::r2ps_response_kafka_message_sender::R2psResponseKafkaMessageSender;
use crate::infrastructure::session_key_memory_cache::SessionKeyMemoryCache;
use crate::infrastructure::{KafkaConfig, R2psRequestKafkaMessageReceiver};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::info;

pub mod application;
pub mod domain;
pub mod infrastructure;

use crate::application::service::device_metadata_service::DeviceMetadataService;
use crate::infrastructure::device_permit_list_memory_cache::DevicePermitListMemoryCache;
use crate::infrastructure::pending_auth_memory_cache::PendingAuthMemoryCache;
use crate::infrastructure::permit_list_kafka_message_receiver::PermitListKafkaMessageReceiver;
// ============ Generate OpenAPI ============
// #[derive(OpenApi)]
// #[openapi(
//     components(schemas(
//         PermitListDto
//     ))
// )]
// pub struct ApiDoc;

pub fn run() {
    // config from env
    infrastructure::config::init();
    let cfg = KafkaConfig::init().unwrap();
    let help = KafkaConfig::get_help();
    info!("{:#?}", help);

    // Handle Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        r.store(false, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    // init server
    let r2ps_kafka_sender = R2psResponseKafkaMessageSender::new(&cfg.clone());
    let client_repository = Arc::new(ClientRepositoryMemoryCache::new());
    let session_key_cache = Arc::new(SessionKeyMemoryCache::new());
    let pending_auth_cache = Arc::new(PendingAuthMemoryCache::new());
    let device_permit_list_cache = Arc::new(DevicePermitListMemoryCache::new());

    let hsm_wrapper = HsmWrapper::new(Pkcs11Config::new_from_env().unwrap()).unwrap();
    let r2ps_service = R2psService::new(
        Arc::new(r2ps_kafka_sender),
        client_repository.clone(),
        session_key_cache.clone(),
        Arc::new(hsm_wrapper),
        pending_auth_cache,
        device_permit_list_cache.clone(),
    );
    let device_metadata_service = Arc::new(DeviceMetadataService::new(
        client_repository,
        device_permit_list_cache.clone(),
    ));

    let r2ps_kafka_receiver = R2psRequestKafkaMessageReceiver::new(&r2ps_service, running.clone());
    let device_permit_list_receiver =
        PermitListKafkaMessageReceiver::new(device_metadata_service.clone(), running.clone());
    // start worker i.e. process requests to responses
    let join_handle = r2ps_kafka_receiver.start_worker_thread(cfg.clone());
    let join_handle2 = device_permit_list_receiver.start_worker_thread(cfg.clone());
    // wait until worker thread finish
    let _ = join_handle.join();
    let _ = join_handle2.join();
}
