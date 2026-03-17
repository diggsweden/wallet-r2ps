use crate::application::service::operations::hsm::MessageVector;
use crate::application::service::operations::hsm::SignatureVector;
use crate::define_byte_vector;
use crate::domain::EcPublicJwk;
use base64::DecodeError;
use serde::{Deserialize, Serialize};
use std::string::FromUtf8Error;
use std::time::Duration;
use strum_macros::Display;
use tracing::warn;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;
use uuid::Uuid;

use super::typed_jwe::TypedJwe;
use super::typed_jws::TypedJws;

/// A unique session identifier (UUID v4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String, format = "uuid"))]
pub struct SessionId(String);

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// The result status of an operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum Status {
    #[serde(rename = "OK")]
    Ok,
    #[serde(rename = "ERROR")]
    Error,
}

/// DTO for HSM worker requests received from Kafka (command topic).
/// State is now server-side — no `state_jws` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerRequestDto {
    /// Server-generated correlation ID
    pub correlation_id: String,
    /// Device identifier
    pub device_id: String,
    /// Client-generated request ID (WebSocket clients only)
    pub request_id: Option<String>,
    /// Optimistic concurrency version
    pub state_version: Option<u64>,
    /// JWS-encoded outer request envelope (OuterRequest)
    pub outer_request_jws: TypedJws<OuterRequest>,
}

/// An HSM worker request — state is now server-owned.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerRequest {
    /// Server-generated correlation ID
    pub correlation_id: String,
    /// Device identifier
    pub device_id: String,
    /// Client-generated request ID (WebSocket clients only)
    pub request_id: Option<String>,
    /// Optimistic concurrency version
    pub state_version: Option<u64>,
    /// JWS-encoded outer request envelope (OuterRequest)
    pub outer_request_jws: TypedJws<OuterRequest>,
}

/// The worker's response — no state_jws, state is server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponse {
    /// Correlation ID matching the original request
    pub correlation_id: String,
    /// Device identifier for Kafka partition key affinity
    pub device_id: String,
    /// Client-generated request ID (WebSocket clients only)
    pub request_id: Option<String>,
    /// JWS-encoded service response (OuterResponse)
    pub outer_response_jws: Option<TypedJws<OuterResponse>>,
    /// The result status of the operation
    pub status: Status,
    /// Error message if the operation failed (serialized JSON)
    pub error_message: Option<String>,
}

/// The outer request envelope, verified via JWS using the device's public key.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct OuterRequest {
    pub version: u32,
    pub session_id: Option<SessionId>,
    pub context: String,
    pub inner_jwe: Option<TypedJwe<InnerRequest>>,
}

/// The decrypted inner request payload.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct InnerRequest {
    pub version: u32,
    #[serde(rename = "type")]
    pub request_type: OperationId,
    pub request_counter: u32,
    pub data: Option<String>,
}

/// The inner response payload returned to the client after decryption.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct InnerResponse {
    pub version: u32,
    pub data: Option<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>, format = "duration"))]
    pub expires_in: Option<iso8601_duration::Duration>,
    pub status: Status,
    pub error_message: Option<String>,
    /// The current HSM state version so clients can track state version
    pub hsm_state_version: Option<u64>,
}

/// The outer response envelope, signed as a JWS by the server.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct OuterResponse {
    pub version: u32,
    pub session_id: Option<SessionId>,
    pub inner_jwe: Option<TypedJwe<InnerResponse>>,
    pub status: Status,
    pub error_message: Option<String>,
}

impl InnerResponse {
    pub fn ok(
        data: String,
        expires_in: Option<iso8601_duration::Duration>,
        hsm_state_version: Option<u64>,
    ) -> Self {
        Self {
            version: 1,
            data: Some(data),
            expires_in,
            status: Status::Ok,
            error_message: None,
            hsm_state_version,
        }
    }

    pub fn error(error_message: String) -> Self {
        Self {
            version: 1,
            data: None,
            expires_in: None,
            status: Status::Error,
            error_message: Some(error_message),
            hsm_state_version: None,
        }
    }
}

impl OuterResponse {
    pub fn ok(inner_jwe: TypedJwe<InnerResponse>, session_id: Option<SessionId>) -> Self {
        Self {
            version: 1,
            inner_jwe: Some(inner_jwe),
            session_id,
            status: Status::Ok,
            error_message: None,
        }
    }

    pub fn error(error_message: String) -> Self {
        Self {
            version: 1,
            inner_jwe: None,
            session_id: None,
            status: Status::Error,
            error_message: Some(error_message),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InnerResponseData {
    data: serde_json::Value,
}

impl InnerResponseData {
    pub fn new<T: Serialize>(data: T) -> Result<Self, ServiceRequestError> {
        serde_json::to_value(data)
            .map(|value| Self { data: value })
            .map_err(|_| ServiceRequestError::Unknown)
    }

    pub fn serialize(&self) -> Result<Vec<u8>, ServiceRequestError> {
        serde_json::to_vec(&self.data).map_err(|_| ServiceRequestError::Unknown)
    }
}

/// Identifies the operation requested by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationId {
    /// Initialize device state (creates version 0)
    StateInit,
    AuthenticateStart,
    AuthenticateFinish,
    RegisterStart,
    RegisterFinish,
    ChangePinStart,
    ChangePinFinish,
    HsmSign,
    HsmEcdh,
    HsmGenerateKey,
    HsmDeleteKey,
    HsmListKeys,
    EndSession,
    Store,
    Retrieve,
    Log,
    GetLog,
    Info,
}

/// Specifies which encryption layer protects the inner JWE payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum EncryptOption {
    Session,
    Device,
}

impl EncryptOption {
    pub fn as_str(&self) -> &'static str {
        match self {
            EncryptOption::Session => "session",
            EncryptOption::Device => "device",
        }
    }
}

impl OperationId {
    pub fn encrypt_option(&self) -> EncryptOption {
        match self {
            OperationId::AuthenticateStart => EncryptOption::Device,
            OperationId::AuthenticateFinish => EncryptOption::Device,
            OperationId::RegisterStart => EncryptOption::Device,
            OperationId::RegisterFinish => EncryptOption::Device,
            OperationId::ChangePinStart => EncryptOption::Session,
            OperationId::ChangePinFinish => EncryptOption::Session,
            OperationId::HsmSign => EncryptOption::Session,
            OperationId::HsmEcdh => EncryptOption::Session,
            OperationId::HsmGenerateKey => EncryptOption::Session,
            OperationId::HsmDeleteKey => EncryptOption::Session,
            OperationId::HsmListKeys => EncryptOption::Session,
            OperationId::EndSession => EncryptOption::Session,
            OperationId::Store => EncryptOption::Session,
            OperationId::Retrieve => EncryptOption::Session,
            OperationId::Log => EncryptOption::Session,
            OperationId::GetLog => EncryptOption::Session,
            OperationId::Info => EncryptOption::Session,
            OperationId::StateInit => EncryptOption::Device,
        }
    }

    /// Returns true for operations that mutate device state.
    pub fn mutates_state(&self) -> bool {
        matches!(
            self,
            OperationId::StateInit
                | OperationId::RegisterFinish
                | OperationId::HsmGenerateKey
                | OperationId::HsmDeleteKey
                | OperationId::ChangePinFinish
        )
    }
}

/// Converts a `std::time::Duration` to an ISO 8601 duration (seconds only)
pub fn to_iso8601_duration(d: Duration) -> iso8601_duration::Duration {
    iso8601_duration::Duration::new(0.0, 0.0, 0.0, 0.0, 0.0, d.as_secs() as f32)
}

define_byte_vector!(PakePayloadVector);

/// Response from a PAKE operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PakeResponse {
    pub task: Option<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>, format = "byte"))]
    pub data: Option<PakePayloadVector>,
}

/// Elliptic curve identifier for key generation.
#[derive(Serialize, Deserialize, Debug, Clone, Display)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum Curve {
    #[serde(rename = "P-256")]
    #[strum(serialize = "P-256")]
    P256,
    #[serde(rename = "P-384")]
    #[strum(serialize = "P-384")]
    P384,
    #[serde(rename = "P-521")]
    #[strum(serialize = "P-521")]
    P521,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CreateKeyServiceData {
    pub curve: Curve,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CreateKeyServiceDataResponse {
    pub public_key: EcPublicJwk,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeleteKeyServiceData {
    pub hsm_kid: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListKeysResponse {
    pub key_info: Vec<KeyInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SignatureResponse {
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub signature: SignatureVector,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct KeyInfo {
    pub created_at: Option<String>,
    pub public_key: EcPublicJwk,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListKeysRequest {}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SignRequest {
    pub hsm_kid: String,
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub message: MessageVector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PakeState {
    Evaluate,
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PakeRequest {
    pub authorization: Option<String>,
    pub task: Option<String>,
    #[serde(rename = "data")]
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub data: PakePayloadVector,
}

impl PakeRequest {
    pub fn from_inner_request(inner_request: InnerRequest) -> Result<Self, ServiceRequestError> {
        let data = inner_request
            .data
            .ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;

        serde_json::from_slice(data.as_bytes()).map_err(|e| {
            warn!("error decoding pake request: {:?}", e);
            ServiceRequestError::InvalidPakeRequest
        })
    }
}

// === New DTOs for state-init and state versioning ===

/// Command DTO for state initialization from BFF (no JWS envelope).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInitCommandDto {
    pub correlation_id: String,
    pub device_id: String,
    pub context: String,
    pub public_key: EcPublicJwk,
}

/// Inner request for state-init (extracted from the command).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct StateInitInnerRequest {
    pub public_key: EcPublicJwk,
}

/// Inner response for state-init.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct StateInitInnerResponse {
    pub dev_authorization_code: String,
    pub device_id: String,
}

/// Event for the state-versions audit topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVersionEvent {
    pub device_id: String,
    pub version: u64,
    pub command_type: String,
    pub correlation_id: String,
    pub timestamp: String,
}

/// Errors that can occur during service request processing.
#[derive(Debug, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum ServiceRequestError {
    JwsError,
    JweError,
    InvalidPakeRequest,
    InvalidRegistrationRequest,
    ServerRegistrationStartFailed,
    ServerLoginStartFailed,
    ServerLoginFinishFailed,
    SerializeResponseError,
    SerializeStateError,
    InvalidServiceRequestFormat,
    InvalidSerializedPasswordFile,
    InvalidAuthenticateRequest,
    UnknownKey,
    UnknownSession,
    UnknownClient,
    InvalidClientPublicKey,
    InvalidPublicKey,
    DuplicateKey,
    HsmKeyNotFound,
    UnsupportedContext,
    InternalServerError,
    InvalidAuthorizationCode,
    Unknown,
    /// Optimistic concurrency conflict
    ConcurrencyConflict,
    /// Device already exists (state-init duplicate)
    ClientAlreadyExists,
    /// Device state not found in DB
    StateNotFound,
    /// Database error
    DatabaseError,
    /// Tamper detection: DB rollback detected
    TamperDetected,
}

impl From<DecodeError> for ServiceRequestError {
    fn from(_: DecodeError) -> Self {
        ServiceRequestError::JweError
    }
}

impl From<FromUtf8Error> for ServiceRequestError {
    fn from(_: FromUtf8Error) -> Self {
        ServiceRequestError::JweError
    }
}

/// Higher-level errors that can occur when processing a worker request.
#[derive(Debug)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum WorkerRequestError {
    ConnectionError,
    ResponseBuildError,
    /// Optimistic concurrency conflict
    ConcurrencyConflict,
    /// Database error
    DatabaseError(String),
    /// Tamper detection failure
    TamperDetected,
}
