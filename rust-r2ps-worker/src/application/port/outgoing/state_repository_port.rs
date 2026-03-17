use serde::{Deserialize, Serialize};
use std::fmt;

/// A versioned device state loaded from the database.
#[derive(Debug, Clone)]
pub struct VersionedState {
    pub state_jws: String,
    pub version: u64,
}

/// An entry to be inserted into the transactional outbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub topic: String,
    pub key: String,
    pub payload: serde_json::Value,
}

/// Errors from the state repository.
#[derive(Debug)]
pub enum StateError {
    NotFound,
    ConcurrencyConflict,
    ClientAlreadyExists,
    DatabaseError(String),
    SerializationError(String),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::NotFound => write!(f, "state not found"),
            StateError::ConcurrencyConflict => write!(f, "concurrency conflict"),
            StateError::ClientAlreadyExists => write!(f, "client already exists"),
            StateError::DatabaseError(msg) => write!(f, "database error: {}", msg),
            StateError::SerializationError(msg) => write!(f, "serialization error: {}", msg),
        }
    }
}

/// Port for persisting device state with transactional outbox guarantees.
pub trait StateRepository: Send + Sync {
    /// Load the current state for a device.
    fn load_current_state(&self, device_id: &str) -> Result<Option<VersionedState>, StateError>;

    /// Atomically persist new state version and outbox entries in a single transaction.
    /// For state-init (version 0→0): `expected_version` is `None`.
    /// For mutations: `expected_version` is `Some(current_version)`.
    fn save_state_with_outbox(
        &self,
        device_id: &str,
        expected_version: Option<u64>,
        new_version: u64,
        state_jws: &str,
        command_type: &str,
        correlation_id: &str,
        outbox_entries: Vec<OutboxEntry>,
    ) -> Result<(), StateError>;
}
