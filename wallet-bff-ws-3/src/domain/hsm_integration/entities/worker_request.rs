use crate::domain::{
    device_management::value_objects::ClientId,
    request_processing::value_objects::{CorrelationId, SignedJws},
};

/// Represents a request to be sent to the HSM worker for processing.
///
/// Encapsulates the data needed by the HSM worker to process a service request:
/// the correlation ID for tracking, the client identity, an optional
/// client-generated request ID (for WebSocket response matching), the current
/// state version for optimistic concurrency, and the signed outer request
/// from the client.
///
/// Device state is no longer sent with requests — the worker loads state
/// server-side from PostgreSQL. The `state_version` enables optimistic
/// concurrency: the worker rejects state-mutating commands if the version
/// is stale.
#[derive(Debug, Clone)]
pub struct WorkerRequest {
    correlation_id: CorrelationId,
    client_id: ClientId,
    request_id: Option<String>,
    state_version: Option<u64>,
    outer_request_jws: SignedJws,
}

impl WorkerRequest {
    pub fn new(
        correlation_id: CorrelationId,
        client_id: ClientId,
        request_id: Option<String>,
        state_version: Option<u64>,
        outer_request_jws: SignedJws,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            request_id,
            state_version,
            outer_request_jws,
        }
    }

    pub fn correlation_id(&self) -> CorrelationId {
        self.correlation_id
    }

    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }

    /// Client-generated request ID for WebSocket response matching.
    /// `None` for REST API requests.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// State version for optimistic concurrency on the worker side.
    /// `None` means the BFF does not enforce version checks (read-only commands).
    pub fn state_version(&self) -> Option<u64> {
        self.state_version
    }

    pub fn outer_request_jws(&self) -> &SignedJws {
        &self.outer_request_jws
    }
}
