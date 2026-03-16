use crate::domain::request_processing::value_objects::CorrelationId;

/// Represents a response received from the HSM worker.
///
/// Contains the processing result: HTTP status and the signed service response.
/// The `client_id` is echoed back from the request so that consumers can route
/// responses without an additional lookup.
///
/// Device state is no longer carried in responses — the worker manages state
/// server-side in PostgreSQL and publishes snapshots to a dedicated topic.
#[derive(Debug, Clone)]
pub struct WorkerResponse {
    correlation_id: CorrelationId,
    client_id: String,
    http_status: u16,
    service_response_jws: String,
}

impl WorkerResponse {
    pub fn new(
        correlation_id: CorrelationId,
        client_id: String,
        http_status: u16,
        service_response_jws: String,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            http_status,
            service_response_jws,
        }
    }

    pub fn correlation_id(&self) -> CorrelationId {
        self.correlation_id
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn http_status(&self) -> u16 {
        self.http_status
    }

    pub fn service_response_jws(&self) -> &str {
        &self.service_response_jws
    }

    pub fn is_success(&self) -> bool {
        self.http_status == 200
    }

    pub fn into_parts(self) -> (CorrelationId, String, u16, String) {
        (
            self.correlation_id,
            self.client_id,
            self.http_status,
            self.service_response_jws,
        )
    }
}
