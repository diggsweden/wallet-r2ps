// SPDX-FileCopyrightText: 2026 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::application::self_test_spi_port::{CheckResult, Outcome, Trigger};
use crate::application::service::{SelfTestService, TsfHealth};
use crate::infrastructure::bootstrap::{build_self_test_probes, build_services};
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::{
    KafkaConfig, PeriodicSelfTestTrigger, StateInitRequestKafkaReceiver, WorkerRequestKafkaReceiver,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{debug, error, info, warn};

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

    let health = TsfHealth::new();

    let services = match build_services(&app_config, kafka_config.clone(), health.clone()) {
        Ok(services) => services,
        Err(e) => {
            log_check_result(&e.into());
            std::process::exit(1);
        }
    };

    let self_test_service = Arc::new(SelfTestService::new(build_self_test_probes(
        &app_config,
        &services,
    )));

    run_and_log_suite(&self_test_service, &health, Trigger::Startup);

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

/// Runs the self-test suite, logs each check result, and applies the outcome to `health`.
/// Shared by the start-up run in `run()` and the periodic trigger so both produce identical
/// logging/gating behavior.
pub(crate) fn run_and_log_suite(
    self_test_service: &SelfTestService,
    health: &TsfHealth,
    trigger: Trigger,
) -> bool {
    let test_results = self_test_service.run_suite(trigger);
    let mut failed: Vec<&str> = Vec::new();

    for result in &test_results {
        log_check_result(result);
        if let Outcome::Fail(_) = &result.outcome {
            failed.push(result.name);
        }
    }
    let healthy = health.apply(&test_results);

    if healthy {
        info!(trigger = ?trigger, total = test_results.len(), healthy ,"self-test suite passed");
    } else {
        error!(trigger = ?trigger, total = test_results.len(), failed = ?failed, healthy ,"self-test suite failed");
    }

    healthy
}

fn log_check_result(result: &CheckResult) {
    match &result.outcome {
        Outcome::Pass => {
            info!(check = result.name, claim = ?result.claim, "self-test check passed")
        }
        Outcome::Fail(e) => {
            error!(check = result.name, claim = ?result.claim, detail = %e.detail, "self-test check failed");
        }
        Outcome::NotImplemented => {
            warn!(check = result.name, claim = ?result.claim, "self-test check not implemented");
        }
    }
}
