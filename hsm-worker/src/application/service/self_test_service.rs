// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;

use crate::application::{
    clock_port::WallClock,
    self_test_spi_port::{CheckResult, Outcome, SelfTestProbe, TsfClaim},
};

pub struct SelfTestService {
    probes: Vec<Arc<dyn SelfTestProbe>>,
    wall_clock: Arc<dyn WallClock>,
}

impl SelfTestService {
    pub fn new(probes: Vec<Arc<dyn SelfTestProbe>>, wall_clock: Arc<dyn WallClock>) -> Self {
        Self { probes, wall_clock }
    }

    /// Runs every probe. The same suite regardless of what triggered the run — the `Trigger`
    /// belongs to the audit record the caller builds, not to the selection of probes.
    pub fn run_suite(&self) -> Vec<CheckResult> {
        let mut results: Vec<CheckResult> = self
            .probes
            .iter()
            .map(|probe| CheckResult {
                name: probe.name(),
                claim: probe.claim(),
                outcome: match probe.probe() {
                    Ok(()) => Outcome::Pass,
                    Err(e) => Outcome::Fail(e),
                },
                at: self.wall_clock.now_utc(),
            })
            .collect();

        for claim in TsfClaim::ALL {
            if !results.iter().any(|r| r.claim == claim) {
                results.push(CheckResult {
                    name: not_implemented_name(claim),
                    claim,
                    outcome: Outcome::NotImplemented,
                    at: self.wall_clock.now_utc(),
                });
            }
        }
        results
    }
}

fn not_implemented_name(claim: TsfClaim) -> &'static str {
    match claim {
        TsfClaim::CryptographicLibraries => "cryptographic_libraries_not_implemented",
        TsfClaim::WscdHsmConnectivity => "wscd_hsm_connectivity_not_implemented",
        TsfClaim::CredentialStoreIntegrity => "credential_store_integrity_not_implemented",
        TsfClaim::AuditLogAvailability => "audit_log_availability_not_implemented",
        TsfClaim::TransactionIdentifierRegistry => {
            "transaction_identifier_registry_not_implemented"
        }
    }
}
