// SPDX-FileCopyrightText: 2026 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerRequestUseCase;
use crate::application::self_test_spi_port::{CheckResult, Outcome, Trigger};
use crate::application::service::{SelfTestService, TsfHealth};
use crate::infrastructure::bootstrap::build_services;
use crate::infrastructure::config::app_config::AppConfig;
use crate::infrastructure::self_test_probes::credential_store_roundtrip::CredentialStoreRoundtripProbe;
use crate::infrastructure::self_test_probes::crypto_a256gcm_kat::CryptoA256GcmKatProbe;
use crate::infrastructure::self_test_probes::crypto_es256_kat::CryptoEs256KatProbe;
use crate::infrastructure::self_test_probes::hsm_roundtrip::HsmRoundtripProbe;
use crate::infrastructure::{
    KafkaConfig, StateInitRequestKafkaReceiver, WorkerRequestKafkaReceiver,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, info};

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

    let services = match build_services(&app_config, kafka_config.clone()) {
        Ok(services) => services,
        Err(e) => {
            log_check_result(&e.into());
            std::process::exit(1);
        }
    };

    let worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync> = Arc::new(services.worker);
    let state_init_service = Arc::new(services.state_init);

    let hsm_roundtrip_probe = HsmRoundtripProbe::new(
        services.hsm.clone(),
        app_config.hsm_root_key_label.clone(),
        app_config.jws_domain_separator.clone(),
    );

    let credential_store_probe = CredentialStoreRoundtripProbe::new(services.session_state.clone());

    let self_test_service = SelfTestService::new(vec![
        Arc::new(CryptoEs256KatProbe),
        Arc::new(CryptoA256GcmKatProbe),
        Arc::new(hsm_roundtrip_probe),
        Arc::new(credential_store_probe),
    ]);
    let trigger = Trigger::Startup;
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
    } else if test_results.is_empty() {
        error!(trigger = ?trigger, healthy, "self-test suite ran no checks; treating as failure");
    } else {
        error!(trigger = ?trigger, total = test_results.len(), failed = ?failed, healthy ,"self-test suite failed");
    }

    // start request worker
    let worker_kafka_receiver = WorkerRequestKafkaReceiver::new(worker_use_case, running.clone());
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

fn log_check_result(result: &CheckResult) {
    match &result.outcome {
        Outcome::Pass => {
            info!(check = result.name, claim = ?result.claim, "self-test check passed")
        }
        Outcome::Fail(e) => {
            error!(check = result.name, claim = ?result.claim, detail = %e.detail, "self-test check failed");
        }
    }
}
