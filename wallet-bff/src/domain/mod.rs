use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// EC P-256 public key in JWK format.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EcPublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
}

/// Request sent by the BFF to the Rust worker via r2ps-requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerRequest {
    pub request_id: String,
    pub state_jws: String,
    pub outer_request_jws: String,
}

/// Response received from the Rust worker via r2ps-responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponse {
    pub request_id: String,
    pub state_jws: Option<String>,
    pub outer_response_jws: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
}

/// Request sent to the state-init-requests topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInitRequest {
    pub request_id: String,
    pub public_key: EcPublicJwk,
}

/// Response received from the state-init-responses topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInitResponse {
    pub request_id: String,
    pub state_jws: String,
    pub dev_authorization_code: String,
    #[serde(default)]
    pub server_jws_public_key: Option<EcPublicJwk>,
    #[serde(default)]
    pub opaque_server_id: Option<String>,
}

/// Pending request metadata stored in Redis, linking a requestId to the device state key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRequestContext {
    pub state_key: String,
    pub ttl_seconds: u64,
}

/// Cached worker response stored in Redis for polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    pub request_id: String,
    pub state_jws: Option<String>,
    pub outer_response_jws: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
}

impl From<WorkerResponse> for CachedResponse {
    fn from(r: WorkerResponse) -> Self {
        Self {
            request_id: r.request_id,
            state_jws: r.state_jws,
            outer_response_jws: r.outer_response_jws,
            status: r.status,
            error_message: r.error_message,
        }
    }
}

// ── HTTP DTOs ────────────────────────────────────────────────────────────────

/// Inbound request body for POST / and POST /service.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BffRequest {
    pub client_id: String,
    pub outer_request_jws: String,
}

/// Inbound request body for POST /new_state.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewStateRequestDto {
    pub public_key: EcPublicJwk,
    pub client_id: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
    pub ttl: Option<String>,
}

/// Response body for POST /new_state.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewStateResponseDto {
    pub status: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_authorization_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_jws_public_key: Option<EcPublicJwk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opaque_server_id: Option<String>,
}

/// Generic async response envelope matching the Java AsyncResponseDto.
/// Fields are omitted when absent to match Java's @JsonInclude(NON_EMPTY).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AsyncResponseDto {
    pub correlation_id: Uuid,
    pub status: AsyncResponseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AsyncResponseError>,
}

/// Status of an async operation.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum AsyncResponseStatus {
    Complete,
    Pending,
    Error,
}

/// Error detail embedded in AsyncResponseDto.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AsyncResponseError {
    pub message: String,
    pub http_status: u16,
}

pub const DEFAULT_TTL_SECONDS: u64 = 600; // 10 minutes, matches r2ps-rest-api default

/// RFC 9457 Problem Details for HTTP APIs.
/// Swedish Dataportal profile: https://www.dataportal.se/rest-api-profil/felhantering
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ProblemDetail {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub problem_type: Option<String>,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}
