use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

use super::connection::RedisConnection;
use crate::domain::{
    device_management::value_objects::ClientId,
    hsm_integration::entities::WorkerResponse,
    request_processing::{errors::RequestError, value_objects::CorrelationId},
};
use crate::ports::outbound::{PendingRequest, RequestRepository};

/// Internal Redis storage model for pending requests.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRequestRecord {
    correlation_id: Uuid,
    client_id: String,
    timestamp: i64,
}

/// Internal Redis storage model for worker responses.
///
/// Device state is no longer stored here — the worker manages state
/// server-side and publishes snapshots to a dedicated topic.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkerResponseRecord {
    request_id: Uuid,
    client_id: String,
    http_status: u16,
    service_response_jws: String,
}

/// Redis-backed implementation of the `RequestRepository` port.
#[derive(Clone)]
pub struct RedisRequestRepository {
    conn: RedisConnection,
    response_ttl: Duration,
}

impl RedisRequestRepository {
    pub fn new(conn: RedisConnection, response_ttl: Duration) -> Self {
        Self { conn, response_ttl }
    }

    fn pending_key(correlation_id: CorrelationId) -> String {
        format!("pending:request:{}", correlation_id)
    }

    fn response_key(correlation_id: CorrelationId) -> String {
        format!("response:r2ps:{}", correlation_id)
    }
}

impl RequestRepository for RedisRequestRepository {
    async fn store_pending(&self, pending: &PendingRequest) -> Result<(), RequestError> {
        let key = Self::pending_key(pending.correlation_id());
        let record = PendingRequestRecord {
            correlation_id: pending.correlation_id().as_uuid(),
            client_id: pending.client_id().to_string(),
            timestamp: pending.timestamp(),
        };
        let json = serde_json::to_string(&record)
            .map_err(|e| RequestError::StorageError(e.to_string()))?;
        let mut conn = self.conn.get();
        conn.set_ex::<_, _, ()>(&key, json, self.response_ttl.as_secs())
            .await
            .map_err(|e| RequestError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn get_pending(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<Option<PendingRequest>, RequestError> {
        let key = Self::pending_key(correlation_id);
        let mut conn = self.conn.get();
        let json: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| RequestError::StorageError(e.to_string()))?;

        match json {
            Some(j) => {
                let record: PendingRequestRecord = serde_json::from_str(&j)
                    .map_err(|e| RequestError::StorageError(e.to_string()))?;
                let client_id = ClientId::new(record.client_id)
                    .map_err(|e| RequestError::StorageError(e.to_string()))?;
                Ok(Some(PendingRequest::new(
                    CorrelationId::from_uuid(record.correlation_id),
                    client_id,
                    record.timestamp,
                )))
            }
            None => Ok(None),
        }
    }

    async fn delete_pending(&self, correlation_id: CorrelationId) -> Result<(), RequestError> {
        let key = Self::pending_key(correlation_id);
        let mut conn = self.conn.get();
        conn.del::<_, ()>(&key)
            .await
            .map_err(|e| RequestError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn store_response(&self, response: &WorkerResponse) -> Result<(), RequestError> {
        let key = Self::response_key(response.correlation_id());
        let record = WorkerResponseRecord {
            request_id: response.correlation_id().as_uuid(),
            client_id: response.client_id().to_string(),
            http_status: response.http_status(),
            service_response_jws: response.service_response_jws().to_string(),
        };
        let json = serde_json::to_string(&record)
            .map_err(|e| RequestError::StorageError(e.to_string()))?;
        let mut conn = self.conn.get();
        conn.set_ex::<_, _, ()>(&key, json, self.response_ttl.as_secs())
            .await
            .map_err(|e| RequestError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn get_response(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<Option<WorkerResponse>, RequestError> {
        let key = Self::response_key(correlation_id);
        let mut conn = self.conn.get();
        let json: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| RequestError::StorageError(e.to_string()))?;

        match json {
            Some(j) => {
                let record: WorkerResponseRecord = serde_json::from_str(&j)
                    .map_err(|e| RequestError::StorageError(e.to_string()))?;
                Ok(Some(WorkerResponse::new(
                    CorrelationId::from_uuid(record.request_id),
                    record.client_id,
                    record.http_status,
                    record.service_response_jws,
                )))
            }
            None => Ok(None),
        }
    }
}
