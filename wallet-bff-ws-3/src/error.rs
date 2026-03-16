use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::domain::{
    device_management::errors::DeviceError, request_processing::errors::RequestError,
};

/// Application-level error type for the HTTP adapter layer.
///
/// Maps domain errors to appropriate HTTP responses using RFC 9457 Problem Details.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("device already exists: {0}")]
    DeviceAlreadyExists(String),

    #[error("request not found: {0}")]
    RequestNotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("timeout waiting for response")]
    Timeout,

    #[error("internal server error: {0}")]
    InternalError(String),
}

impl AppError {
    /// Map a `DeviceError` from the domain layer to an `AppError` for HTTP responses.
    pub fn from_device_error(err: DeviceError) -> Self {
        match err {
            DeviceError::NotFound(msg) => AppError::DeviceNotFound(msg),
            DeviceError::AlreadyExists(msg) => AppError::DeviceAlreadyExists(msg),
            DeviceError::InvalidClientId(e) => AppError::InvalidRequest(e.to_string()),
            DeviceError::InvalidDeviceState(e) => AppError::InvalidRequest(e.to_string()),
            DeviceError::InvalidPublicKey(e) => AppError::InvalidRequest(e.to_string()),
            DeviceError::StorageError(msg) => {
                if msg.contains("timeout") {
                    AppError::Timeout
                } else {
                    AppError::InternalError(msg)
                }
            }
        }
    }

    /// Map a `RequestError` from the domain layer to an `AppError` for HTTP responses.
    pub fn from_request_error(err: RequestError) -> Self {
        match err {
            RequestError::NotFound(msg) => AppError::RequestNotFound(msg),
            RequestError::InvalidJws(e) => AppError::InvalidRequest(e.to_string()),
            RequestError::Timeout => AppError::Timeout,
            RequestError::StorageError(msg) => AppError::InternalError(msg),
            RequestError::MessagingError(msg) => AppError::InternalError(msg),
        }
    }
}

/// RFC 9457 Problem Details for HTTP APIs (FEL.01, FEL.02).
/// Complies with Swedish REST API Profile v1.2.0.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "type": "https://api.digg.se/r2ps/v1/problems/invalid-request",
    "title": "Invalid Request",
    "status": 400,
    "detail": "The request body is malformed or missing required fields"
}))]
pub struct ProblemDetail {
    /// A URI reference that identifies the problem type
    #[serde(rename = "type")]
    pub problem_type: String,

    /// A short, human-readable summary of the problem type
    pub title: String,

    /// The HTTP status code
    pub status: u16,

    /// A human-readable explanation specific to this occurrence
    pub detail: String,

    /// A URI reference that identifies the specific occurrence of the problem
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, problem_type, title, detail) = match self {
            AppError::DeviceNotFound(ref msg) => (
                StatusCode::NOT_FOUND,
                "device-not-found",
                "Device Not Found",
                msg.clone(),
            ),
            AppError::DeviceAlreadyExists(ref msg) => (
                StatusCode::CONFLICT,
                "device-already-exists",
                "Device Already Exists",
                msg.clone(),
            ),
            AppError::RequestNotFound(ref msg) => (
                StatusCode::NOT_FOUND,
                "request-not-found",
                "Request Not Found",
                msg.clone(),
            ),
            AppError::InvalidRequest(ref msg) => (
                StatusCode::BAD_REQUEST,
                "invalid-request",
                "Invalid Request",
                msg.clone(),
            ),
            AppError::Timeout => (
                StatusCode::REQUEST_TIMEOUT,
                "request-timeout",
                "Request Timeout",
                "The request timed out while waiting for a response from the HSM worker"
                    .to_string(),
            ),
            AppError::InternalError(ref _msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal-error",
                "Internal Server Error",
                // Don't expose internal details (SAK.25, SAK.26)
                "An internal server error occurred".to_string(),
            ),
        };

        // Build RFC 9457 Problem Details response
        let problem_detail = ProblemDetail {
            problem_type: format!("https://api.digg.se/r2ps/v1/problems/{}", problem_type),
            title: title.to_string(),
            status: status.as_u16(),
            detail,
            instance: None,
        };

        // Return with proper Content-Type: application/problem+json (FEL.02)
        let mut response = (status, Json(problem_detail)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            "application/problem+json".parse().unwrap(),
        );

        response
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
