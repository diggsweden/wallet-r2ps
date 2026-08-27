// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use rstest::rstest;

use crate::application::{
    self_test_spi_port::{CheckResult, Outcome::Pass, SelfTestError, TsfClaim},
    service::TsfHealth,
};

fn pass(name: &'static str) -> CheckResult {
    CheckResult {
        name,
        claim: TsfClaim::CryptographicLibraries,
        outcome: Pass,
    }
}

fn fail(name: &'static str) -> CheckResult {
    CheckResult {
        name,
        claim: TsfClaim::CryptographicLibraries,
        outcome: crate::application::self_test_spi_port::Outcome::Fail(SelfTestError {
            detail: "forced".to_string(),
        }),
    }
}

#[test]
fn a_new_health_flag_starts_unhealthy() {
    assert!(!TsfHealth::new().is_healthy())
}

#[rstest]
#[case::empty(vec![], false)]
#[case::all_passing(vec![pass("a"), pass("b"), pass("c")], true)]
#[case::single_failure_at_start(vec![fail("a"), pass("b"), pass("c")], false)]
#[case::single_failure_in_middle(vec![pass("a"), fail("b"), pass("c")], false)]
#[case::all_failing(vec![fail("a"), fail("b"), fail("c")], false)]
fn apply_reports_healthy_only_when_every_check_passes(
    #[case] results: Vec<CheckResult>,
    #[case] expected_healthy: bool,
) {
    let tsf_health = TsfHealth::new();

    assert_eq!(tsf_health.apply(&results), expected_healthy);
    assert_eq!(tsf_health.is_healthy(), expected_healthy)
}

#[test]
fn health_is_restored_when_a_later_run_passes() {
    let tsf_health = TsfHealth::new();
    tsf_health.apply(&[fail("a")]);
    assert!(!tsf_health.is_healthy());

    tsf_health.apply(&[pass("a")]);
    assert!(tsf_health.is_healthy())
}

// this test is just to make sure any future refactor don't break the
// share-on-clone behavior of TsfHealth.
#[test]
fn a_clone_observes_the_same_state() {
    let tsf_health = TsfHealth::new();
    let tsf_health_clone = tsf_health.clone();

    tsf_health.apply(&[pass("a")]);
    assert!(tsf_health_clone.is_healthy());

    tsf_health.apply(&[fail("a")]);
    assert!(!tsf_health_clone.is_healthy())
}

#[test]
fn quartantine_marks_healthy_tsf_unhealthy() {
    let tsf_health = TsfHealth::new();
    tsf_health.apply(&[pass("a")]);

    tsf_health.quarantine();

    assert!(!tsf_health.is_healthy())
}
