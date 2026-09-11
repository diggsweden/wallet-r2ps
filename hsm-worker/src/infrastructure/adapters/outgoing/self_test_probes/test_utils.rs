// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Duration;

use hsm_common::SessionId;

use crate::{
    application::session_state_spi_port::{
        SessionState, SessionStateError, SessionStateSpiPort, SessionTransition,
    },
    infrastructure::session_state_memory_cache::SessionStateMemoryCache,
};

pub struct FakeSessionStore {
    /// 'None' models a store that acknowledges writes without persisting them.
    inner: Option<SessionStateMemoryCache>,
    reject: fn(&SessionTransition) -> bool,
}

fn reject_nothing(_: &SessionTransition) -> bool {
    false
}

impl FakeSessionStore {
    /// Acknowledges every write but persists nothing - a store that lies.
    pub fn acks_without_persisting() -> Self {
        Self {
            inner: None,
            reject: reject_nothing,
        }
    }

    /// Real cache that rejects the transitions matching 'reject'.
    pub fn rejecting(reject: fn(&SessionTransition) -> bool) -> Self {
        Self {
            inner: Some(SessionStateMemoryCache::new()),
            reject,
        }
    }
}

impl SessionStateSpiPort for FakeSessionStore {
    fn get(&self, id: &SessionId) -> Option<SessionState> {
        self.inner.as_ref()?.get(id)
    }

    fn apply_transition(
        &self,
        session_id: Option<&SessionId>,
        transition: Option<&SessionTransition>,
    ) -> Result<(), SessionStateError> {
        if transition.is_some_and(self.reject) {
            return Err(SessionStateError::InvalidTransition);
        }
        match &self.inner {
            Some(cache) => cache.apply_transition(session_id, transition),
            None => Ok(()),
        }
    }

    fn get_remaining_ttl(&self, session_id: Option<&SessionId>) -> Option<Duration> {
        self.inner.as_ref()?.get_remaining_ttl(session_id)
    }
}
