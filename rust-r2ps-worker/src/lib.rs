use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use crate::application::R2psService;
use crate::infrastructure::client_repository_memory_cache::ClientRepositoryMemoryCache;
use crate::infrastructure::{KafkaConfig, R2psRequestKafkaMessageReceiver};
use crate::infrastructure::hsm_wrapper::{HsmWrapper, Pkcs11Config};
use crate::infrastructure::r2ps_response_kafka_message_sender::R2psResponseKafkaMessageSender;
use crate::infrastructure::session_key_memory_cache::SessionKeyMemoryCache;

pub mod domain;
pub mod infrastructure;
pub mod application;

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
    }).expect("Error setting Ctrl-C handler");

    // init server
    let r2ps_kafka_sender = R2psResponseKafkaMessageSender::new(&cfg.clone());
    let client_repository = Arc::new(ClientRepositoryMemoryCache::new());
    let session_key_cache = Arc::new(SessionKeyMemoryCache::new());
    let hsm_wrapper = HsmWrapper::new(Pkcs11Config::new_from_env().unwrap()).unwrap();
    let r2ps_service = R2psService::new(Arc::new(r2ps_kafka_sender), client_repository.clone(), session_key_cache.clone(), Arc::new(hsm_wrapper));
    let r2ps_kafka_receiver = R2psRequestKafkaMessageReceiver::new(&r2ps_service, running.clone());

    // start worker i.e. process requests to responses
    let join_handle = r2ps_kafka_receiver.start_worker_thread(cfg);

    // wait until worker thread finish
    let _ = join_handle.join();
}