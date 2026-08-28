// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::session_state_spi_port::{
    PendingLoginState, SessionKey, SessionState, SessionStateError, SessionStateSpiPort,
    SessionTransition,
};
use crate::domain::SessionId;
use crate::infrastructure::adapters::outgoing::session_state_memory_cache::SessionStateMemoryCache;

#[test]
fn session_state_cache_preserves_purpose_on_pendingauth_to_active() {
    let cache = SessionStateMemoryCache::new();
    let id = SessionId::new();
    let purpose = Some("wallet-access".to_string());

    cache
        .apply_transition(
            Some(&id),
            Some(&SessionTransition::CreatePendingAuth {
                pending_state: PendingLoginState::new(vec![1]),
                purpose: purpose.clone(),
            }),
        )
        .unwrap();

    cache
        .apply_transition(
            Some(&id),
            Some(&SessionTransition::Authenticate {
                session_key: SessionKey::new(vec![0u8; 32]),
            }),
        )
        .unwrap();

    let state = cache
        .get(&id)
        .expect("session must exist after authenticate");
    let SessionState::Active(data) = state else {
        panic!("expected Active state");
    };
    assert_eq!(data.purpose, purpose);
}

#[test]
fn session_state_cache_apply_transition_requires_session_id_when_transition_present() {
    let cache = SessionStateMemoryCache::new();
    let result = cache.apply_transition(None, Some(&SessionTransition::End));
    assert!(matches!(result, Err(SessionStateError::UnknownSession)));
}

#[test]
fn second_mark_hsm_operation_is_rejected_by_the_cache() {
    let cache = SessionStateMemoryCache::new();
    let id = SessionId::new();
    cache
        .apply_transition(
            Some(&id),
            Some(&SessionTransition::CreatePendingAuth {
                pending_state: PendingLoginState::new(vec![0u8; 8]),
                purpose: None,
            }),
        )
        .unwrap();
    cache
        .apply_transition(
            Some(&id),
            Some(&SessionTransition::Authenticate {
                session_key: SessionKey::new(vec![0u8; 4]),
            }),
        )
        .unwrap();

    assert!(
        cache
            .apply_transition(
                Some(&id),
                Some(&SessionTransition::MarkHsmOperationPerformed)
            )
            .is_ok()
    );
    assert!(
        cache
            .apply_transition(
                Some(&id),
                Some(&SessionTransition::MarkHsmOperationPerformed)
            )
            .is_err()
    );
}
