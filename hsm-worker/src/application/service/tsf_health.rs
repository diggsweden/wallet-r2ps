// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Whether the TSF has demonstrated correct operation.
//!
//! `FPT_TST.1` Application Note 30 (TR 4.3 p. 33) — "Failed self-tests shall result in
//! refusal to process RAC requests" — and `FPT_FLS.1.1` (p. 32). This flag is the state
//! that refusal is derived from; the refusal itself is the guard at the top of
//! `WorkerService::process_request` / `StateInitService::initialize`, chosen over gating
//! the Kafka subscription.
//!
//! Any failed check makes the TSF unhealthy. The PP describes no tolerated failure: §8.2.3
//! (p. 40) requires the WSCA-BE to fail closed and leaves availability to HSM redundancy and
//! operator SLAs.
//!
//! Two-way by design. `FPT_RCV.4` and Table 7 / T.TSF_FAILURE (p. 39) require recovery to a
//! secure state, so a later passing run must be able to lift a quarantine.
//!
//! The periodic self-test trigger writes this flag from its own thread for the lifetime of the
//! process, while the worker and state-init threads read it on every request to decide whether
//! to refuse. `Release`/`Acquire` ordering makes the store happen-before the load it's meant to
//! gate.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    Arc,
    application::self_test_spi_port::{CheckResult, Outcome},
};

#[derive(Clone, Debug)]
pub struct TsfHealth {
    healthy: Arc<AtomicBool>,
}

impl TsfHealth {
    pub fn new() -> Self {
        Self {
            healthy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    pub fn apply(&self, results: &[CheckResult]) -> bool {
        let healthy = !results.is_empty()
            && results
                .iter()
                .all(|r| matches!(r.outcome, Outcome::Pass | Outcome::NotImplemented));
        self.healthy.store(healthy, Ordering::Release);
        healthy
    }

    pub fn quarantine(&self) {
        self.healthy.store(false, Ordering::Release);
    }
}

impl Default for TsfHealth {
    fn default() -> Self {
        Self::new()
    }
}
