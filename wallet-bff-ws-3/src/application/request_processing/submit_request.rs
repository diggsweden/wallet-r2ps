use std::time::Duration;

use chrono::Utc;

use crate::domain::{
    device_management::value_objects::ClientId,
    hsm_integration::entities::WorkerRequest,
    request_processing::{
        errors::RequestError,
        value_objects::{CorrelationId, ProcessingMode, SignedJws},
    },
};
use crate::ports::outbound::{
    ClientKeyRepository, MessagePublisher, PendingRequest, RequestRepository,
};

/// Result of submitting a service request.
#[derive(Debug)]
pub enum SubmitResult {
    /// Request accepted and pending (async mode).
    Pending { correlation_id: CorrelationId },
    /// Request completed with a response (sync mode).
    Complete {
        correlation_id: CorrelationId,
        response_jws: String,
    },
    /// Request completed with an error from the HSM worker.
    Failed {
        correlation_id: CorrelationId,
        http_status: u16,
        message: String,
    },
}

/// Use case: Submit a service request for HSM processing.
///
/// Orchestrates the flow:
/// 1. Validate device exists (via client key cache)
/// 2. Create correlation context
/// 3. Send request to HSM worker (no device state — worker loads from DB)
/// 4. Return immediately (async) or wait for response (sync)
pub struct SubmitRequestUseCase<K, R, M>
where
    K: ClientKeyRepository,
    R: RequestRepository,
    M: MessagePublisher,
{
    key_repo: K,
    request_repo: R,
    publisher: M,
    sync_timeout: Duration,
}

impl<K, R, M> SubmitRequestUseCase<K, R, M>
where
    K: ClientKeyRepository,
    R: RequestRepository,
    M: MessagePublisher,
{
    pub fn new(key_repo: K, request_repo: R, publisher: M, sync_timeout: Duration) -> Self {
        Self {
            key_repo,
            request_repo,
            publisher,
            sync_timeout,
        }
    }

    pub async fn execute(
        &self,
        client_id_str: &str,
        outer_request_jws: &str,
        request_id: Option<String>,
        mode: ProcessingMode,
    ) -> Result<SubmitResult, RequestError> {
        let client_id =
            ClientId::new(client_id_str).map_err(|e| RequestError::StorageError(e.to_string()))?;

        // Validate device exists (via client key cache populated by state-snapshot consumer)
        let device_exists = self
            .key_repo
            .exists(&client_id)
            .await
            .map_err(|e| RequestError::StorageError(e.to_string()))?;

        if !device_exists {
            return Err(RequestError::NotFound(format!(
                "Device {} not found",
                client_id
            )));
        }

        let outer_jws = SignedJws::new(outer_request_jws)?;

        // Generate correlation ID and store pending context
        let correlation_id = CorrelationId::generate();
        let pending =
            PendingRequest::new(correlation_id, client_id.clone(), Utc::now().timestamp());
        self.request_repo.store_pending(&pending).await?;

        // Build worker request — no device state, no version
        // The worker loads state from its own PostgreSQL database
        let worker_request =
            WorkerRequest::new(correlation_id, client_id, request_id, None, outer_jws);

        // Send to HSM worker
        self.publisher
            .publish_worker_request(&worker_request)
            .await
            .map_err(|e| RequestError::MessagingError(e.to_string()))?;

        // Return based on processing mode
        if mode.is_sync() {
            self.wait_for_response(correlation_id).await
        } else {
            Ok(SubmitResult::Pending { correlation_id })
        }
    }

    async fn wait_for_response(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<SubmitResult, RequestError> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(100);

        loop {
            if let Some(response) = self.request_repo.get_response(correlation_id).await? {
                return Ok(self.map_worker_response(correlation_id, &response));
            }

            if start.elapsed() >= self.sync_timeout {
                return Err(RequestError::Timeout);
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    fn map_worker_response(
        &self,
        correlation_id: CorrelationId,
        response: &crate::domain::hsm_integration::entities::WorkerResponse,
    ) -> SubmitResult {
        if response.is_success() {
            SubmitResult::Complete {
                correlation_id,
                response_jws: response.service_response_jws().to_string(),
            }
        } else {
            SubmitResult::Failed {
                correlation_id,
                http_status: response.http_status(),
                message: "Request failed".to_string(),
            }
        }
    }
}
