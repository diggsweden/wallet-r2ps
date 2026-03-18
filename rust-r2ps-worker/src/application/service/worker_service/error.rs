use crate::domain::ServiceRequestError;
use problem_details::ProblemDetails;
use serde_json::{Map, Value, json};
use std::fmt;

/// Errors only reported to the calling microservice — nothing sent to client.
#[derive(Debug, Clone)]
pub enum UpstreamError {
    InvalidStateJws,
    OuterJwsInvalid,
    OuterJwsMissingKid,
    UnknownDevice,
    EncodeFailed(&'static str),
}

/// Errors sent to the client in the (unencrypted) outer response.
/// Safe to expose because they reveal nothing about inner content.
#[derive(Debug, Clone)]
pub enum OuterError {
    InnerJweMissing,
    InnerJweHeaderInvalid,
    InnerJweDecryptFailed,
    UnknownEncryptionOption,
    SessionKeyMissing,
    UnsupportedContext,
}

/// An error tagged by who can see it on the return path.
/// The variant itself is the visibility — no separate routing needed.
#[derive(Debug, Clone)]
pub enum WorkerError {
    /// Microservice only — client receives nothing.
    Upstream(UpstreamError),
    /// Client sees error in the outer (unencrypted) response.
    Outer(OuterError),
    /// Client sees error inside the encrypted inner response only.
    Inner(ServiceRequestError),
}

impl From<UpstreamError> for WorkerError {
    fn from(e: UpstreamError) -> Self {
        WorkerError::Upstream(e)
    }
}

impl From<OuterError> for WorkerError {
    fn from(e: OuterError) -> Self {
        WorkerError::Outer(e)
    }
}

impl From<ServiceRequestError> for WorkerError {
    fn from(e: ServiceRequestError) -> Self {
        WorkerError::Inner(e)
    }
}

/// Implemented by each error type so they can produce a problem details JSON.
/// The default impl uses `{:?}` to derive the detail string from the enum variant name.
pub trait ProblemDetail: fmt::Debug {
    fn to_problem_details_json(&self, request_id: &str) -> String {
        let mut extensions: Map<String, Value> = Map::new();
        extensions.insert("request_id".to_string(), json!(request_id));

        let details = ProblemDetails::new()
            .with_title("Error processing request")
            .with_detail(format!("{self:?}"))
            .with_extensions(extensions);

        serde_json::to_string(&details).unwrap_or_else(|_| "{}".to_string())
    }
}

impl ProblemDetail for UpstreamError {}
impl ProblemDetail for OuterError {}
impl ProblemDetail for ServiceRequestError {}

impl ProblemDetail for WorkerError {
    fn to_problem_details_json(&self, request_id: &str) -> String {
        match self {
            Self::Upstream(e) => e.to_problem_details_json(request_id),
            Self::Outer(e) => e.to_problem_details_json(request_id),
            Self::Inner(e) => e.to_problem_details_json(request_id),
        }
    }
}
