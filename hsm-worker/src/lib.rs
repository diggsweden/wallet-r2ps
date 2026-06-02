// SPDX-FileCopyrightText: 2026 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::infrastructure::bootstrap::build_services;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::{
    KafkaConfig, StateInitRequestKafkaReceiver, WorkerRequestKafkaReceiver,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use tracing::{debug, info};

pub mod application;
pub mod domain;
pub mod infrastructure;

pub fn run() {
    // config from env
    let app_config = AppConfig::new().unwrap();

    let kafka_config: Arc<KafkaConfig> = Arc::new(app_config.clone().into());

    assert!(
        kafka_config.consumer_threads_request >= 1,
        "kafka_consumer_threads_request must be >= 1"
    );
    assert!(
        kafka_config.consumer_threads_state_init >= 1,
        "kafka_consumer_threads_state_init must be >= 1"
    );

    // Handle Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        debug!("Received shutdown signal");
        r.store(false, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    let (worker_service, state_init_service) = build_services(&app_config, kafka_config.clone());
    let worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync> = Arc::new(worker_service);
    let state_init_service = Arc::new(state_init_service);

    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    // start request workers
    for i in 0..kafka_config.consumer_threads_request {
        let receiver =
            WorkerRequestKafkaReceiver::new(worker_use_case.clone(), running.clone());
        handles.push(receiver.start_worker_thread(kafka_config.clone(), i));
    }

    // start state init request workers
    for i in 0..kafka_config.consumer_threads_state_init {
        let receiver =
            StateInitRequestKafkaReceiver::new(state_init_service.clone(), running.clone());
        handles.push(receiver.start_worker_thread(kafka_config.clone(), i));
    }

    info!(
        "HSM worker started (request threads: {}, state-init threads: {})",
        kafka_config.consumer_threads_request, kafka_config.consumer_threads_state_init
    );

    // wait until all worker threads finish
    for h in handles {
        let _ = h.join();
    }
}
