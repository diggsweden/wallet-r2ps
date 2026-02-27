use crate::infrastructure::bootstrap::build_services;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::{
    KafkaConfig, StateInitRequestKafkaReceiver, WorkerRequestKafkaReceiver,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info};

pub mod application;
pub mod domain;
pub mod infrastructure;

pub fn run() {
    // config from env
    let app_config = AppConfig::new().unwrap();

    let kafka_config: Arc<KafkaConfig> = Arc::new(app_config.clone().into());

    // Handle Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        debug!("Received shutdown signal");
        r.store(false, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    let (worker_service, state_init_service) = build_services(&app_config, kafka_config.clone());
    let worker_service = Arc::new(worker_service);
    let state_init_service = Arc::new(state_init_service);

    // start request worker
    let worker_kafka_receiver = WorkerRequestKafkaReceiver::new(worker_service, running.clone());
    let join_handle = worker_kafka_receiver.start_worker_thread(kafka_config.clone());

    // start state init request worker
    let state_init_receiver =
        StateInitRequestKafkaReceiver::new(state_init_service, running.clone());
    let state_init_handle = state_init_receiver.start_worker_thread(kafka_config.clone());

    info!("HSM worker started");

    // wait until both worker threads finish
    let _ = join_handle.join();
    let _ = state_init_handle.join();
}
