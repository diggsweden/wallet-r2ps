// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::Duration;

use crate::application::self_test_spi_port::Trigger;
use crate::application::service::{PeriodicScheduler, SelfTestService, TsfHealth};
use crate::infrastructure::adapters::outgoing::system_clock::SystemClock;
use crate::run_and_log_suite;

/// How often the shutdown flag is re-checked while waiting for the next periodic run;
/// mirrors the 100ms poll cadence the Kafka receivers use for shutdown responsiveness.
const POLL_TICK: Duration = Duration::from_millis(500);

/// Drives `FPT_TST.1.1`'s periodic self-test run: runs the self-test suite on a fixed interval
/// for as long as the process is up, independent of the start-up run in `run()`.
pub struct PeriodicSelfTestTrigger {
    self_test_service: Arc<SelfTestService>,
    health: TsfHealth,
    running: Arc<AtomicBool>,
    interval: Duration,
}

impl PeriodicSelfTestTrigger {
    pub fn new(
        self_test_service: Arc<SelfTestService>,
        health: TsfHealth,
        running: Arc<AtomicBool>,
        interval: Duration,
    ) -> Self {
        Self {
            self_test_service,
            health,
            running,
            interval,
        }
    }

    pub fn start_worker_thread(self) -> JoinHandle<()> {
        spawn(move || {
            let mut scheduler = PeriodicScheduler::new(SystemClock, self.interval);

            while self.running.load(Ordering::Relaxed) {
                if scheduler.due() {
                    run_and_log_suite(&self.self_test_service, &self.health, Trigger::Periodic);
                }
                std::thread::sleep(POLL_TICK);
            }
        })
    }
}
