// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use chrono::{DateTime, Utc};
use rstest::rstest;

use crate::application::self_test_spi_port::{
    CheckResult, Outcome, SelfTestError, Trigger, TsfClaim,
};
use crate::application::service::{AuditEvent, TsfHealth};

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn check(name: &'static str, outcome: Outcome, at: &str) -> CheckResult {
    CheckResult {
        name,
        claim: TsfClaim::CryptographicLibraries,
        outcome,
        at: ts(at),
    }
}

fn failure(detail: &str) -> Outcome {
    Outcome::Fail(SelfTestError {
        detail: detail.to_string(),
    })
}

#[test]
fn failing_check_record_carries_the_probe_detail_and_the_checks_own_time() {
    let result = check(
        "kat_ecdsa_p256",
        failure("signature mismatch"),
        "2026-01-01T12:00:03Z",
    );

    assert_eq!(
        AuditEvent::for_check(Trigger::Periodic, &result),
        AuditEvent {
            at: ts("2026-01-01T12:00:03Z"),
            event_type: "kat_ecdsa_p256",
            trigger: "periodic",
            claim: Some("cryptographic_libraries"),
            subject: "system",
            outcome: "fail",
            detail: Some("signature mismatch".to_string()),
            total: None,
            failed: None,
        }
    );
}

#[test]
fn passing_check_record_carries_no_detail() {
    let result = check("kat_ecdsa_p256", Outcome::Pass, "2026-01-01T12:00:03Z");

    let event = AuditEvent::for_check(Trigger::Startup, &result);

    assert_eq!(event.outcome, "pass");
    assert_eq!(event.detail, None);
}

#[test]
fn verdict_record_counts_every_check_and_the_failures_separately() {
    let results = vec![
        check("a", Outcome::Pass, "2026-01-01T12:00:00Z"),
        check("b", failure("hsm unreachable"), "2026-01-01T12:00:01Z"),
        check("c", failure("wrap key missing"), "2026-01-01T12:00:02Z"),
    ];

    assert_eq!(
        AuditEvent::for_verdict(Trigger::Startup, ts("2026-01-01T12:00:03Z"), &results),
        AuditEvent {
            at: ts("2026-01-01T12:00:03Z"),
            event_type: "self_test_suite",
            trigger: "startup",
            claim: None,
            subject: "system",
            outcome: "fail",
            detail: None,
            total: Some(3),
            failed: Some(2),
        }
    );
}

#[rstest]
#[case::all_passed(vec![Outcome::Pass, Outcome::Pass], "pass")]
#[case::coverage_gaps_are_not_failures(vec![Outcome::Pass, Outcome::NotImplemented], "pass")]
#[case::a_single_failure_fails_the_suite(vec![Outcome::Pass, failure("boom")], "fail")]
#[case::a_suite_that_ran_nothing_never_passes(vec![], "fail")]
fn verdict_outcome_agrees_with_the_health_rule(
    #[case] outcomes: Vec<Outcome>,
    #[case] expected: &str,
) {
    let results: Vec<CheckResult> = outcomes
        .into_iter()
        .map(|outcome| check("c", outcome, "2026-01-01T12:00:00Z"))
        .collect();

    let event = AuditEvent::for_verdict(Trigger::Startup, ts("2026-01-01T12:00:01Z"), &results);

    assert_eq!(event.outcome, expected);
    assert_eq!(event.outcome == "pass", TsfHealth::new().apply(&results));
}
