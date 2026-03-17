use crate::application::port::outgoing::state_repository_port::{
    OutboxEntry, StateError, StateRepository, VersionedState,
};
use postgres::{Client, NoTls};
use std::sync::Mutex;
use tracing::{debug, error, info};

/// PostgreSQL-backed state repository with transactional outbox support.
pub struct PostgresStateRepository {
    client: Mutex<Client>,
}

impl PostgresStateRepository {
    pub fn new(connection_string: &str) -> Result<Self, String> {
        let client = Client::connect(connection_string, NoTls).map_err(|e| {
            error!("Failed to connect to PostgreSQL: {:?}", e);
            format!("PostgreSQL connection failed: {}", e)
        })?;

        info!("Connected to PostgreSQL");

        Ok(Self {
            client: Mutex::new(client),
        })
    }
}

impl StateRepository for PostgresStateRepository {
    fn load_current_state(&self, device_id: &str) -> Result<Option<VersionedState>, StateError> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| StateError::DatabaseError(format!("mutex poisoned: {}", e)))?;

        let row = client
            .query_opt(
                "SELECT dsv.state_jws, dsv.version
                 FROM device_state_head dsh
                 JOIN device_state_version dsv ON dsv.device_id = dsh.device_id
                                                 AND dsv.version = dsh.current_version
                 WHERE dsh.device_id = $1",
                &[&device_id],
            )
            .map_err(|e| StateError::DatabaseError(format!("query failed: {}", e)))?;

        match row {
            Some(row) => {
                let state_jws: String = row.get(0);
                let version: i64 = row.get(1);

                debug!(
                    "Loaded state for device_id={}, version={}",
                    device_id, version
                );

                Ok(Some(VersionedState {
                    state_jws,
                    version: version as u64,
                }))
            }
            None => {
                debug!("No state found for device_id={}", device_id);
                Ok(None)
            }
        }
    }

    fn save_state_with_outbox(
        &self,
        device_id: &str,
        expected_version: Option<u64>,
        new_version: u64,
        state_jws: &str,
        command_type: &str,
        correlation_id: &str,
        outbox_entries: Vec<OutboxEntry>,
    ) -> Result<(), StateError> {
        let mut client = self
            .client
            .lock()
            .map_err(|e| StateError::DatabaseError(format!("mutex poisoned: {}", e)))?;

        let mut tx = client
            .transaction()
            .map_err(|e| StateError::DatabaseError(format!("begin transaction failed: {}", e)))?;

        // Optimistic concurrency: SELECT FOR UPDATE on device_state_head
        match expected_version {
            None => {
                // State-init: INSERT new head (fails if device already exists)
                let result = tx.execute(
                    "INSERT INTO device_state_head (device_id, current_version)
                     VALUES ($1, $2)
                     ON CONFLICT (device_id) DO NOTHING",
                    &[&device_id, &(new_version as i64)],
                );
                match result {
                    Ok(0) => {
                        // Row already exists → device already initialized
                        return Err(StateError::ClientAlreadyExists);
                    }
                    Ok(_) => {} // Successfully inserted
                    Err(e) => {
                        return Err(StateError::DatabaseError(format!(
                            "insert device_state_head failed: {}",
                            e
                        )));
                    }
                }
            }
            Some(expected) => {
                // Mutation: SELECT FOR UPDATE with version check
                let row = tx
                    .query_opt(
                        "SELECT current_version FROM device_state_head
                         WHERE device_id = $1 FOR UPDATE",
                        &[&device_id],
                    )
                    .map_err(|e| {
                        StateError::DatabaseError(format!("select for update failed: {}", e))
                    })?;

                match row {
                    None => return Err(StateError::NotFound),
                    Some(row) => {
                        let current: i64 = row.get(0);
                        if current as u64 != expected {
                            return Err(StateError::ConcurrencyConflict);
                        }
                    }
                }

                // Update the head
                tx.execute(
                    "UPDATE device_state_head SET current_version = $1, updated_at = now()
                     WHERE device_id = $2",
                    &[&(new_version as i64), &device_id],
                )
                .map_err(|e| {
                    StateError::DatabaseError(format!("update device_state_head failed: {}", e))
                })?;
            }
        }

        // INSERT into device_state_version (append-only event log)
        tx.execute(
            "INSERT INTO device_state_version
             (device_id, version, state_jws, command_type, correlation_id)
             VALUES ($1, $2, $3, $4, $5)",
            &[
                &device_id,
                &(new_version as i64),
                &state_jws,
                &command_type,
                &correlation_id,
            ],
        )
        .map_err(|e| {
            StateError::DatabaseError(format!("insert device_state_version failed: {}", e))
        })?;

        // INSERT outbox entries
        for entry in &outbox_entries {
            let payload_json: serde_json::Value = entry.payload.clone();
            tx.execute(
                "INSERT INTO outbox (topic, key, payload) VALUES ($1, $2, $3)",
                &[&entry.topic, &entry.key, &payload_json],
            )
            .map_err(|e| StateError::DatabaseError(format!("insert outbox failed: {}", e)))?;
        }

        // COMMIT
        tx.commit()
            .map_err(|e| StateError::DatabaseError(format!("commit failed: {}", e)))?;

        debug!(
            "Persisted state version {} for device_id={} with {} outbox entries",
            new_version,
            device_id,
            outbox_entries.len()
        );

        Ok(())
    }
}
