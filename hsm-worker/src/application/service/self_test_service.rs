// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;

use crate::application::self_test_spi_port::{
    CheckResult, Outcome, SelfTestProbe, Trigger, TsfClaim,
};

pub struct SelfTestService {
    probes: Vec<Arc<dyn SelfTestProbe>>,
}

impl SelfTestService {
    pub fn new(probes: Vec<Arc<dyn SelfTestProbe>>) -> Self {
        Self { probes }
    }

    pub fn run_suite(&self, _trigger: Trigger) -> Vec<CheckResult> {
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
            })
            .collect();

        for claim in TsfClaim::ALL {
            if !results.iter().any(|r| r.claim == claim) {
                results.push(CheckResult {
                    name: not_implemented_name(claim),
                    claim,
                    outcome: Outcome::NotImplemented,
                });
            }
        }
        results
    }
}

fn not_implemented_name(claim: TsfClaim) -> &'static str {
    match claim {
        TsfClaim::CryptographicLibraries => "cryptographic-libraries-not-implemented",
        TsfClaim::WscdHsmConnectivity => "wscd-hsm-connectivity-not-implemented",
        TsfClaim::CredentialStoreIntegrity => "credential-store-integrity-not-implemented",
        TsfClaim::AuditLogAvailability => "audit-log-availability-not-implemented",
        TsfClaim::TransactionIdentifierRegistry => {
            "transaction-identifier-registry-not-implemented"
        }
    }
}
