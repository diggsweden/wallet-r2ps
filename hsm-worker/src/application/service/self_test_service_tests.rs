// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::self_test_spi_port::{
    CheckResult, Outcome, SelfTestError, SelfTestProbe, Trigger, TsfClaim,
};
use crate::application::service::SelfTestService;
use std::sync::Arc;

struct FakeProbe {
    name: &'static str,
    claim: TsfClaim,
    outcome: Result<(), String>,
}

impl SelfTestProbe for FakeProbe {
    fn name(&self) -> &'static str {
        self.name
    }

    fn claim(&self) -> TsfClaim {
        self.claim
    }

    fn probe(&self) -> Result<(), SelfTestError> {
        self.outcome
            .clone()
            .map_err(|detail| SelfTestError { detail })
    }
}

fn pass(name: &'static str) -> Arc<dyn SelfTestProbe> {
    Arc::new(FakeProbe {
        name,
        claim: TsfClaim::CryptographicLibraries,
        outcome: Ok(()),
    })
}

fn fail(name: &'static str, detail: &str) -> Arc<dyn SelfTestProbe> {
    Arc::new(FakeProbe {
        name,
        claim: TsfClaim::CryptographicLibraries,
        outcome: Err(detail.to_string()),
    })
}

#[test]
fn empty_probe_list_report_every_claim_not_implemented() {
    let service = SelfTestService::new(vec![]);
    let results = service.run_suite(Trigger::Startup);

    assert_eq!(results.len(), TsfClaim::ALL.len());
    assert!(
        results
            .iter()
            .all(|r| matches!(r.outcome, Outcome::NotImplemented))
    )
}

#[test]
fn one_passing_probe_gives_one_pass_and_leaves_other_claims_not_implemented() {
    let service = SelfTestService::new(vec![pass("a")]);

    let results = service.run_suite(Trigger::Startup);

    let passed: Vec<&CheckResult> = results
        .iter()
        .filter(|r| r.outcome == Outcome::Pass)
        .collect();

    assert_eq!(passed.len(), 1);
    assert_eq!(passed[0].name, "a");
    assert_eq!(passed[0].claim, TsfClaim::CryptographicLibraries);

    let not_implemented = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::NotImplemented))
        .count();
    assert_eq!(not_implemented, TsfClaim::ALL.len() - 1);
}

#[test]
fn mixed_probes_preserve_order_and_carry_name_claim_outcome() {
    let service = SelfTestService::new(vec![fail("b", "boom"), pass("a")]);

    let results = service.run_suite(Trigger::Startup);

    assert_eq!(
        &results[..2],
        vec![
            CheckResult {
                name: "b",
                claim: TsfClaim::CryptographicLibraries,
                outcome: Outcome::Fail(SelfTestError {
                    detail: "boom".to_string(),
                }),
            },
            CheckResult {
                name: "a",
                claim: TsfClaim::CryptographicLibraries,
                outcome: Outcome::Pass,
            },
        ]
    );

    assert_eq!(results.len(), 2 + TsfClaim::ALL.len() - 1);
}
