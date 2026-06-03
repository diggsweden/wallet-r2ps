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
        kafka_config.request_worker_tasks >= 1,
        "hsm_worker_tasks_request must be >= 1"
    );
    assert!(
        kafka_config.state_init_worker_tasks >= 1,
        "hsm_worker_tasks_state_init must be >= 1"
    );
    assert!(
        kafka_config.request_worker_queue_depth >= 1,
        "hsm_worker_queue_depth_request must be >= 1"
    );
    assert!(
        kafka_config.state_init_worker_queue_depth >= 1,
        "hsm_worker_queue_depth_state_init must be >= 1"
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

    // One Kafka consumer per topic per pod; N workers per topic drain the
    // dispatch channels and call into the application layer.
    let request_receiver =
        WorkerRequestKafkaReceiver::new(worker_use_case.clone(), running.clone());
    handles.extend(request_receiver.start(
        kafka_config.clone(),
        kafka_config.request_worker_tasks,
        kafka_config.request_worker_queue_depth,
    ));

    let state_init_receiver =
        StateInitRequestKafkaReceiver::new(state_init_service.clone(), running.clone());
    handles.extend(state_init_receiver.start(
        kafka_config.clone(),
        kafka_config.state_init_worker_tasks,
        kafka_config.state_init_worker_queue_depth,
    ));

    info!(
        "HSM worker started (request workers: {}, state-init workers: {}, request queue: {}, state-init queue: {})",
        kafka_config.request_worker_tasks,
        kafka_config.state_init_worker_tasks,
        kafka_config.request_worker_queue_depth,
        kafka_config.state_init_worker_queue_depth,
    );

    // wait until all consumer + worker threads finish
    for h in handles {
        let _ = h.join();
    }
}
