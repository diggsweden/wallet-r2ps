// SPDX-FileCopyrightText: 2026 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::application::clock_port::WallClock;
use crate::application::self_test_spi_port::Trigger;
use crate::application::service::audit_event::emit;
use crate::application::service::{AuditEvent, SelfTestService, TsfHealth};
use crate::infrastructure::bootstrap::{build_self_test_probes, build_services};
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::system_clock::SystemClock;
use crate::infrastructure::{
    KafkaConfig, PeriodicSelfTestTrigger, StateInitRequestKafkaReceiver, WorkerRequestKafkaReceiver,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{debug, info};

pub mod application;
pub mod domain;
pub mod infrastructure;

pub fn run() {
    // config from env
    let app_config = AppConfig::new().unwrap();

    let kafka_config: Arc<KafkaConfig> = Arc::new(app_config.clone().into());

    let wall_clock: Arc<dyn WallClock> = Arc::new(SystemClock);

    // Handle Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        debug!("Received shutdown signal");
        r.store(false, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    let health = TsfHealth::new();

    let services = match build_services(&app_config, kafka_config.clone(), health.clone()) {
        Ok(services) => services,
        Err(e) => {
            let result = e.into_check_result(wall_clock.now_utc());
            emit(&AuditEvent::for_check(Trigger::Startup, &result));
            std::process::exit(1);
        }
    };

    let self_test_service = Arc::new(SelfTestService::new(
        build_self_test_probes(&app_config, &services),
        wall_clock,
    ));

    self_test_service.run_and_report(&health, Trigger::Startup);

    let worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync> = Arc::new(services.worker);
    let state_init_service = Arc::new(services.state_init);

    // start request worker
    let worker_kafka_receiver = WorkerRequestKafkaReceiver::new(worker_use_case, running.clone());
    let join_handle = worker_kafka_receiver.start_worker_thread(kafka_config.clone());

    // start state init request worker
    let state_init_receiver =
        StateInitRequestKafkaReceiver::new(state_init_service, running.clone());
    let state_init_handle = state_init_receiver.start_worker_thread(kafka_config.clone());

    // start periodic self-test trigger (FPT_TST.1.1)
    let periodic_self_test_trigger = PeriodicSelfTestTrigger::new(
        self_test_service,
        health,
        running.clone(),
        Duration::from_secs(app_config.self_test_periodic_interval_secs),
    );
    let periodic_self_test_handle = periodic_self_test_trigger.start_worker_thread();

    info!("HSM worker started");

    // wait until all worker threads finish
    let _ = join_handle.join();
    let _ = state_init_handle.join();
    let _ = periodic_self_test_handle.join();
}
