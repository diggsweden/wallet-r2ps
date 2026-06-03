// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::session_state_spi_port::{
    OngoingOperation, PendingAuthData, SessionData, SessionState, SessionStateError,
    SessionStateSpiPort, SessionTransition,
};
use crate::domain::SessionId;
use moka::sync::Cache;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

const SESSION_TTL_SECS: u64 = 600;

#[derive(Clone)]
struct CacheEntry {
    state: SessionState,
    inserted_at: Instant,
}

pub struct SessionStateMemoryCache {
    cache: Cache<SessionId, CacheEntry>,
}

impl Default for SessionStateMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStateMemoryCache {
    pub fn new() -> Self {
        let cache = Cache::builder()
            .time_to_live(Duration::from_secs(SESSION_TTL_SECS))
            .max_capacity(100_000)
            .build();
        Self { cache }
    }

    fn insert(&self, id: &SessionId, state: SessionState) {
        self.cache.insert(
            id.clone(),
            CacheEntry {
                state,
                inserted_at: Instant::now(),
            },
        );
    }
}

enum NextState {
    Set(SessionState),
    Invalidate,
}

/// Pure FSM transition function. Computes the next session state given the current state
/// and a requested transition. Returns `Err(InvalidTransition)` for any combination not
/// listed below — including future transitions that have not yet been implemented.
///
/// Valid transitions:
///
/// | Current state                                    | Transition          | Next state                        |
/// |--------------------------------------------------|---------------------|-----------------------------------|
/// | None                                             | CreatePendingAuth   | PendingAuth                       |
/// | PendingAuth                                      | Authenticate        | Active { operation: None }        |
/// | Active { operation: None }                       | BeginChangingPin    | Active { operation: ChangingPin } |
/// | Active { operation: None, hsm_op: false }         | MarkHsmOperationPerformed | Active { hsm_op: true }     |
/// | Active { .. }                                    | End                 | None (invalidate)                 |
/// |--------------------------------------------------|---------------------|-----------------------------------|
fn next_state(
    current: Option<SessionState>,
    transition: SessionTransition,
) -> Result<NextState, SessionStateError> {
    match (current, transition) {
        // New auth flow started; no prior session must exist
        (
            None,
            SessionTransition::CreatePendingAuth {
                pending_state,
                purpose,
            },
        ) => Ok(NextState::Set(SessionState::PendingAuth(PendingAuthData {
            server_login: pending_state,
            purpose,
        }))),

        // OPAQUE login completed; promote pending auth to active session, carrying over purpose
        (
            Some(SessionState::PendingAuth(pending)),
            SessionTransition::Authenticate { session_key },
        ) => Ok(NextState::Set(SessionState::Active(SessionData {
            session_key,
            purpose: pending.purpose,
            operation: None,
            has_performed_hsm_operation: false,
        }))),

        // Pin change initiated; only valid when no other operation is already in progress
        (Some(SessionState::Active(data)), SessionTransition::BeginChangingPin)
            if data.operation.is_none() =>
        {
            Ok(NextState::Set(SessionState::Active(SessionData {
                operation: Some(OngoingOperation::ChangingPin),
                ..data
            })))
        }

        // HSM operation completed; only valid when no operation in progress and no prior HSM op
        (Some(SessionState::Active(data)), SessionTransition::MarkHsmOperationPerformed)
            if data.operation.is_none() && !data.has_performed_hsm_operation =>
        {
            Ok(NextState::Set(SessionState::Active(SessionData {
                has_performed_hsm_operation: true,
                ..data
            })))
        }

        // Session ended; valid from any active state
        (Some(SessionState::Active(_)), SessionTransition::End) => Ok(NextState::Invalidate),

        // All other combinations are invalid
        _ => Err(SessionStateError::InvalidTransition),
    }
}

impl SessionStateSpiPort for SessionStateMemoryCache {
    fn get(&self, id: &SessionId) -> Option<SessionState> {
        let state = self.cache.get(id).map(|e| e.state);
        debug!("Session {:?} state: {:#?}", id, state);
        state
    }

    fn apply_transition(
        &self,
        session_id: Option<&SessionId>,
        transition: Option<&SessionTransition>,
    ) -> Result<(), SessionStateError> {
        let Some(transition) = transition.cloned() else {
            return Ok(());
        };
        let id = session_id.ok_or(SessionStateError::UnknownSession)?;
        let current = self.cache.get(id).map(|e| e.state);
        debug!(session_id = ?id, current_state = ?current, transition = ?transition, "Applying session transition");

        match next_state(current, transition).inspect_err(|_| {
            warn!(session_id = ?id, "Invalid session state transition");
        })? {
            NextState::Set(state) => self.insert(id, state),
            NextState::Invalidate => self.cache.invalidate(id),
        }

        Ok(())
    }

    fn get_remaining_ttl(&self, session_id: Option<&SessionId>) -> Option<Duration> {
        const TTL: Duration = Duration::from_secs(SESSION_TTL_SECS);
        self.cache.get(session_id?).and_then(|entry| {
            let elapsed = entry.inserted_at.elapsed();
            TTL.checked_sub(elapsed)
        })
    }
}
