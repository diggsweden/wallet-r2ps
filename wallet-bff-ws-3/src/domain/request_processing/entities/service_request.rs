use crate::domain::{
    device_management::value_objects::ClientId,
    request_processing::value_objects::{CorrelationId, SignedJws},
};
use chrono::{DateTime, Utc};

/// Aggregate root for the Request Processing bounded context.
///
/// Represents a service request submitted by a device for HSM processing.
/// Tracks the request lifecycle from submission through completion.
#[derive(Debug, Clone)]
pub struct ServiceRequest {
    correlation_id: CorrelationId,
    client_id: ClientId,
    outer_request_jws: SignedJws,
    created_at: DateTime<Utc>,
    status: RequestStatus,
}

/// The lifecycle status of a service request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestStatus {
    /// Request has been submitted and is awaiting HSM processing.
    Pending,
    /// HSM worker returned a successful response.
    Complete { response_jws: SignedJws },
    /// HSM worker returned an error.
    Failed { http_status: u16, message: String },
}

impl ServiceRequest {
    /// Create a new service request.
    pub fn new(client_id: ClientId, outer_request_jws: SignedJws) -> Self {
        Self {
            correlation_id: CorrelationId::generate(),
            client_id,
            outer_request_jws,
            created_at: Utc::now(),
            status: RequestStatus::Pending,
        }
    }

    pub fn correlation_id(&self) -> CorrelationId {
        self.correlation_id
    }

    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }

    pub fn outer_request_jws(&self) -> &SignedJws {
        &self.outer_request_jws
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn status(&self) -> &RequestStatus {
        &self.status
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.status, RequestStatus::Pending)
    }

    /// Mark the request as completed with a successful response.
    pub fn complete(&mut self, response_jws: SignedJws) {
        self.status = RequestStatus::Complete { response_jws };
    }

    /// Mark the request as failed.
    pub fn fail(&mut self, http_status: u16, message: String) {
        self.status = RequestStatus::Failed {
            http_status,
            message,
        };
    }
}
