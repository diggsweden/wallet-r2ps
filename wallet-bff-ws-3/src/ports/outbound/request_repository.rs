use crate::domain::{
    device_management::value_objects::ClientId,
    hsm_integration::entities::WorkerResponse,
    request_processing::{errors::RequestError, value_objects::CorrelationId},
};

/// Stored context for a pending request.
///
/// Tracks which device submitted the request so that the HSM response
/// handler can update the correct device state.
#[derive(Debug, Clone)]
pub struct PendingRequest {
    correlation_id: CorrelationId,
    client_id: ClientId,
    timestamp: i64,
}

impl PendingRequest {
    pub fn new(correlation_id: CorrelationId, client_id: ClientId, timestamp: i64) -> Self {
        Self {
            correlation_id,
            client_id,
            timestamp,
        }
    }

    pub fn correlation_id(&self) -> CorrelationId {
        self.correlation_id
    }

    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

/// Port for persisting and retrieving request-related data.
///
/// Covers pending request tracking and response caching.
pub trait RequestRepository: Send + Sync {
    /// Store a pending request context.
    fn store_pending(
        &self,
        pending: &PendingRequest,
    ) -> impl Future<Output = Result<(), RequestError>> + Send;

    /// Retrieve a pending request by correlation ID.
    fn get_pending(
        &self,
        correlation_id: CorrelationId,
    ) -> impl Future<Output = Result<Option<PendingRequest>, RequestError>> + Send;

    /// Delete a pending request.
    fn delete_pending(
        &self,
        correlation_id: CorrelationId,
    ) -> impl Future<Output = Result<(), RequestError>> + Send;

    /// Store an HSM worker response.
    fn store_response(
        &self,
        response: &WorkerResponse,
    ) -> impl Future<Output = Result<(), RequestError>> + Send;

    /// Retrieve a stored HSM worker response.
    fn get_response(
        &self,
        correlation_id: CorrelationId,
    ) -> impl Future<Output = Result<Option<WorkerResponse>, RequestError>> + Send;
}
