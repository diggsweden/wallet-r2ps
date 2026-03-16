use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::connection::RedisConnection;
use crate::domain::hsm_integration::{entities::StateInitResponse, errors::HsmError};
use crate::ports::outbound::StateInitRepository;

/// Internal Redis storage model for state init responses.
///
/// State-init responses now arrive as regular worker responses on `r2ps-responses`.
/// They are stored temporarily by correlation_id so the synchronous
/// `InitializeDeviceUseCase` can poll for them.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateInitRecord {
    correlation_id: String,
    http_status: u16,
    service_response_jws: String,
}

/// Redis-backed implementation of the `StateInitRepository` port.
#[derive(Clone)]
pub struct RedisStateInitRepository {
    conn: RedisConnection,
}

impl RedisStateInitRepository {
    pub fn new(conn: RedisConnection) -> Self {
        Self { conn }
    }

    fn key(correlation_id: &str) -> String {
        format!("response:state_init:{}", correlation_id)
    }
}

impl StateInitRepository for RedisStateInitRepository {
    async fn store_response(&self, response: &StateInitResponse) -> Result<(), HsmError> {
        let key = Self::key(response.correlation_id());
        let record = StateInitRecord {
            correlation_id: response.correlation_id().to_string(),
            http_status: response.http_status(),
            service_response_jws: response.service_response_jws().to_string(),
        };
        let json =
            serde_json::to_string(&record).map_err(|e| HsmError::StorageError(e.to_string()))?;
        let mut conn = self.conn.get();
        conn.set_ex::<_, _, ()>(&key, json, 10u64) // 10 second TTL
            .await
            .map_err(|e| HsmError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn get_response(
        &self,
        correlation_id: &str,
    ) -> Result<Option<StateInitResponse>, HsmError> {
        let key = Self::key(correlation_id);
        let mut conn = self.conn.get();
        let json: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| HsmError::StorageError(e.to_string()))?;

        match json {
            Some(j) => {
                let record: StateInitRecord =
                    serde_json::from_str(&j).map_err(|e| HsmError::StorageError(e.to_string()))?;
                Ok(Some(StateInitResponse::new(
                    record.correlation_id,
                    record.http_status,
                    record.service_response_jws,
                )))
            }
            None => Ok(None),
        }
    }

    async fn wait_for_response(
        &self,
        correlation_id: &str,
        timeout: Duration,
    ) -> Result<Option<StateInitResponse>, HsmError> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            if let Some(response) = self.get_response(correlation_id).await? {
                return Ok(Some(response));
            }

            if start.elapsed() >= timeout {
                return Ok(None);
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}
