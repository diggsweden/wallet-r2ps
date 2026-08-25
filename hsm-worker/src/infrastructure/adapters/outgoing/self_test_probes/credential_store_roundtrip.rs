// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! FPT_TST.1 Application Note 30 — "credential store integrity".
//!
//! This is a liveness/round-trip check, not an integrity check: it proves the live
//! `SessionStateSpiPort` implementation can write, read, report a TTL and invalidate a
//! session — it does not detect unauthorised modification or rollback, so it does not
//! satisfy FDP_SDI.2 (TR 4.3 p. 30, Application Note 20).
//!
//! Exercises the same `Arc<dyn SessionStateSpiPort>` the running `WorkerService` holds.
//! `End` is only a valid transition from `Active` (`session_state_memory_cache.rs`'s
//! `next_state`), so reaching it requires driving `CreatePendingAuth -> Authenticate ->
//! End` — there is no shortcut from `PendingAuth` straight to cleanup.

use std::{sync::Arc, time::Duration};

use hsm_common::SessionId;

use crate::application::{
    self_test_spi_port::{SelfTestError, SelfTestProbe, TsfClaim},
    session_state_spi_port::{
        PendingLoginState, SessionKey, SessionState, SessionStateSpiPort, SessionTransition,
    },
};

pub struct CredentialStoreRoundtripProbe {
    session_state: Arc<dyn SessionStateSpiPort>,
}

impl CredentialStoreRoundtripProbe {
    pub fn new(session_state: Arc<dyn SessionStateSpiPort>) -> Self {
        Self { session_state }
    }
}

impl SelfTestProbe for CredentialStoreRoundtripProbe {
    fn name(&self) -> &'static str {
        "credential_store_roundtrip"
    }

    fn claim(&self) -> TsfClaim {
        TsfClaim::CredentialStoreIntegrity
    }

    fn probe(&self) -> Result<(), SelfTestError> {
        let session_id = SessionId::new();
        let mut first_failure: Option<String> = None;
        let mut record = |ok: bool, detail: &str| {
            if !ok && first_failure.is_none() {
                first_failure = Some(detail.to_string());
            }
        };

        // 1. write
        if let Err(e) = self.session_state.apply_transition(
            Some(&session_id),
            Some(&SessionTransition::CreatePendingAuth {
                pending_state: PendingLoginState::new(vec![0u8; 8]),
                purpose: None,
            }),
        ) {
            // Nothing was written: nothing to clean up
            return Err(SelfTestError {
                detail: format!("credential_store_roundtrip: create pending auth: {e:?}"),
            });
        }

        // 2. read
        record(
            matches!(
                self.session_state.get(&session_id),
                Some(SessionState::PendingAuth(_))
            ),
            "expected PendingAuth after write, got something else",
        );

        // 3. TTL
        let ttl_ok = matches!(
        self.session_state.get_remaining_ttl(Some(&session_id)),
        Some(d) if d > Duration::ZERO && d <= Duration::from_secs(600)
        );
        record(ttl_ok, "TTL missing or out of expected bounds after write");

        // 4. progress to Active - the only state End is valid from
        if let Err(e) = self.session_state.apply_transition(
            Some(&session_id),
            Some(&SessionTransition::Authenticate {
                session_key: SessionKey::new(vec![0u8; 4]),
            }),
        ) {
            return Err(SelfTestError {
                detail: first_failure.unwrap_or_else(|| {
                    format!("credential_store_roundtrip: authenticate transition: {e:?}")
                }),
            });
        }
        record(
            matches!(
                self.session_state.get(&session_id),
                Some(SessionState::Active(_))
            ),
            "expected Active after authenticate, got something else",
        );

        // 5. end - always attempted once Active is reached, regardless of earlier failures
        if let Err(e) = self
            .session_state
            .apply_transition(Some(&session_id), Some(&SessionTransition::End))
        {
            record(false, &format!("end transition rejected: {e:?}"));
        }

        // 6. absent
        record(
            self.session_state.get(&session_id).is_none(),
            "session still present after End",
        );

        match first_failure {
            Some(detail) => Err(SelfTestError {
                detail: format!("credential_store_roundtrip: {detail}"),
            }),
            None => Ok(()),
        }
    }
}
