use crate::domain::hsm_integration::{entities::WorkerResponse, errors::HsmError};
use crate::ports::outbound::RequestRepository;

/// Use case: Process an incoming response from the HSM worker.
///
/// Stores the response so it can be polled by the client.
///
/// Device state updates are no longer handled here — the worker manages
/// state server-side in PostgreSQL and publishes snapshots to the
/// `state-snapshot` topic.
pub struct ProcessWorkerResponseUseCase<R>
where
    R: RequestRepository,
{
    request_repo: R,
}

impl<R> ProcessWorkerResponseUseCase<R>
where
    R: RequestRepository,
{
    pub fn new(request_repo: R) -> Self {
        Self { request_repo }
    }

    pub async fn execute(&self, response: WorkerResponse) -> Result<(), HsmError> {
        let correlation_id = response.correlation_id();

        // Store the response for polling
        self.request_repo
            .store_response(&response)
            .await
            .map_err(|e| HsmError::StorageError(e.to_string()))?;

        tracing::info!(
            correlation_id = %correlation_id,
            "Stored worker response"
        );

        Ok(())
    }
}
