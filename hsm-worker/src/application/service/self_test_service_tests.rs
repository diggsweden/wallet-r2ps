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
fn empty_probe_list_gives_empty_result() {
    let service = SelfTestService::new(vec![]);
    assert_eq!(service.run_suite(Trigger::Startup), vec![]);
}

#[test]
fn one_passing_probe_gives_one_passing_result() {
    let service = SelfTestService::new(vec![pass("a")]);

    let results = service.run_suite(Trigger::Startup);

    assert_eq!(
        results,
        vec![CheckResult {
            name: "a",
            claim: TsfClaim::CryptographicLibraries,
            outcome: Outcome::Pass,
        }]
    );
}

#[test]
fn mixed_probes_preserve_order_and_carry_name_claim_outcome() {
    let service = SelfTestService::new(vec![fail("b", "boom"), pass("a")]);

    let results = service.run_suite(Trigger::Startup);

    assert_eq!(
        results,
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
}
