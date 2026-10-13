// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Self-test audit records — FAU_GEN.1.1(i) ("all self-test results") in the shape
//! FAU_GEN.1.2 mandates: date and time, type of event, subject identity, outcome.

use chrono::{DateTime, SecondsFormat, Utc};
use tracing::{error, info, warn};

use crate::application::self_test_spi_port::{CheckResult, Outcome, Trigger};

// Audit vocabulary, like `Trigger::as_str` and `Outcome::as_str`: an evaluator and any log
// query filter on these values, so they must not change when a Rust identifier is renamed.

/// FAU_GEN.1.2 subject identity of a TSF-initiated run. Every `Trigger` variant is started by
/// the TOE itself, with no requesting user, so the subject is constant rather than optional.
/// A user-initiated run carries the requester instead (FAU_GEN.2.1).
const SUBJECT_SYSTEM: &str = "system";

/// Event type of the suite-verdict record. Distinct from the per-check event types because the
/// verdict is separately the FPT_FLS.1-relevant event: reading it should not require folding
/// every check record back together.
const SELF_TEST_SUITE: &str = "self_test_suite";

/// One audit record: one per check, plus one per suite run carrying the verdict.
#[derive(Debug, PartialEq, Eq)]
pub struct AuditEvent {
    /// FAU_GEN.1.2 date and time — when the check ran, or when the suite finished.
    pub at: DateTime<Utc>,
    /// FAU_GEN.1.2 type of event: the check name, or `self_test_suite` for the verdict.
    pub event_type: &'static str,
    /// What caused the run.
    pub trigger: &'static str,
    /// The Application Note 30 item the check evidences. `None` on the verdict, which spans
    /// every claim.
    pub claim: Option<&'static str>,
    /// FAU_GEN.1.2 subject identity.
    pub subject: &'static str,
    /// FAU_GEN.1.2 outcome: `pass`, `fail` or `not_implemented`.
    pub outcome: &'static str,
    /// Why a check failed. Never key material, PINs or OPAQUE state — see `SelfTestError`.
    pub detail: Option<String>,
    /// Checks run, and of those how many failed. `None` on a per-check record, which describes
    /// a single check.
    pub total: Option<usize>,
    pub failed: Option<usize>,
}

impl AuditEvent {
    /// One record per check, stamped with the check's own time rather than the suite's: a probe
    /// that hangs is visible in the trail, which a single suite-level timestamp would hide.
    pub fn for_check(trigger: Trigger, result: &CheckResult) -> Self {
        AuditEvent {
            at: result.at,
            event_type: result.name,
            trigger: trigger.as_str(),
            claim: Some(result.claim.as_str()),
            subject: SUBJECT_SYSTEM,
            outcome: result.outcome.as_str(),
            detail: match &result.outcome {
                Outcome::Fail(e) => Some(e.detail.clone()),
                Outcome::Pass | Outcome::NotImplemented => None,
            },
            total: None,
            failed: None,
        }
    }

    /// The suite verdict, stamped by the caller once every check has run.
    ///
    /// Passing requires at least one check and no failure — the rule `TsfHealth::apply` uses to
    /// decide whether the TSF quarantines itself, so record and quarantine state agree.
    /// `NotImplemented` is not a failure: it reports absent AN 30 coverage, not a broken TSF.
    pub fn for_verdict(trigger: Trigger, at: DateTime<Utc>, results: &[CheckResult]) -> Self {
        let failed = results
            .iter()
            .filter(|r| matches!(r.outcome, Outcome::Fail(_)))
            .count();
        let healthy = !results.is_empty() && failed == 0;

        AuditEvent {
            at,
            event_type: SELF_TEST_SUITE,
            trigger: trigger.as_str(),
            claim: None,
            subject: SUBJECT_SYSTEM,
            // Spelled out because `Outcome::as_str` needs an `Outcome`, and a verdict over many
            // checks is not one. The values must stay identical to what it returns.
            outcome: if healthy { "pass" } else { "fail" },
            detail: None,
            total: Some(results.len()),
            failed: Some(failed),
        }
    }
}

/// Writes `event` as a single `tracing` event on `target: "audit"`, one field per record field.
/// The dedicated target keeps audit records routable and filterable apart from diagnostics.
pub fn emit(event: &AuditEvent) {
    let at = event.at.to_rfc3339_opts(SecondsFormat::Millis, true);

    match event.outcome {
        "fail" => error!(
            target: "audit",
            at = %at,
            event_type = event.event_type,
            trigger = event.trigger,
            claim = event.claim,
            subject = event.subject,
            outcome = event.outcome,
            detail = event.detail.as_deref(),
            total = event.total,
            failed = event.failed,
            "self-test audit record"
        ),
        "not_implemented" => warn!(
            target: "audit",
            at = %at,
            event_type = event.event_type,
            trigger = event.trigger,
            claim = event.claim,
            subject = event.subject,
            outcome = event.outcome,
            detail = event.detail.as_deref(),
            total = event.total,
            failed = event.failed,
            "self-test audit record"
        ),
        _ => info!(
            target: "audit",
            at = %at,
            event_type = event.event_type,
            trigger = event.trigger,
            claim = event.claim,
            subject = event.subject,
            outcome = event.outcome,
            detail = event.detail.as_deref(),
            total = event.total,
            failed = event.failed,
            "self-test audit record"
        ),
    }
}
