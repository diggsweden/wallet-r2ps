use crate::application::service::operations::hsm::MessageVector;
use crate::application::service::operations::hsm::SignatureVector;
use crate::define_byte_vector;
use crate::domain::EcPublicJwk;
use base64::DecodeError;
use josekit::JoseError;
use pem::Pem;
use serde::{Deserialize, Serialize};
use std::string::FromUtf8Error;
use std::time::Duration;
use strum_macros::Display;
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    #[serde(rename = "OK")]
    Ok,
    #[serde(rename = "ERROR")]
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct R2psRequestDto {
    pub request_id: String,
    pub wallet_id: String, // remove later? device_id or client_id?
    pub device_id: String, // remove later? device_id or client_id?
    pub state_jws: String,
    pub service_request_jws: String,
}

// Define your output message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct R2psResponseDto {
    pub request_id: String,
    pub wallet_id: String, // remove later? device_id or client_id?
    pub device_id: String, // remove later? device_id or client_id?
    pub http_status: u16,
    pub state_jws: String,
    pub service_response_jws: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct R2psRequestJws {
    pub request_id: String,
    pub wallet_id: String, // remove later? device_id or client_id?
    pub device_id: String, // remove later? device_id or client_id?
    pub state_jws: String,
    pub outer_request_jws: String,
}

// Define your output message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct R2psResponseJws {
    pub request_id: String,
    pub wallet_id: String, // remove later? device_id or client_id?
    pub device_id: String, // remove later? device_id or client_id?
    pub http_status: u16,
    pub state_jws: String,
    pub service_response_jws: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OuterRequest {
    pub version: u32,
    pub session_id: Option<SessionId>,
    pub context: String, // always "hsm". TODO: Replace with JOSE "aud" header?
    pub inner_jwe: Option<super::InnerJwe>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InnerRequest {
    pub version: u32,
    #[serde(rename = "type")]
    pub request_type: OperationId,
    pub request_counter: u32, // TODO: Implement replay protection using this counter
    pub data: Option<String>, // request specific data, serialized JSON typically
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InnerResponse {
    pub version: u32,
    pub data: Option<String>, // request specific response data, serialized JSON typically
    pub expires_in: Option<iso8601_duration::Duration>, // time until session expires
    pub status: Status,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OuterResponse {
    pub version: u32,
    pub session_id: Option<SessionId>,
    pub inner_jwe: Option<super::InnerJwe>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationId {
    AuthenticateStart,
    AuthenticateFinish,
    RegisterStart,
    RegisterFinish,
    PinChange,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
            OperationId::PinChange => EncryptOption::Session,
            OperationId::HsmSign => EncryptOption::Session,
            OperationId::HsmEcdh => EncryptOption::Session,
            OperationId::HsmGenerateKey => EncryptOption::Session,
            OperationId::HsmDeleteKey => EncryptOption::Session,
            OperationId::HsmListKeys => EncryptOption::Session,
            OperationId::EndSession => EncryptOption::Device, // TODO: Why is this Device?
            OperationId::Store => EncryptOption::Session,
            OperationId::Retrieve => EncryptOption::Session,
            OperationId::Log => EncryptOption::Session,
            OperationId::GetLog => EncryptOption::Session,
            OperationId::Info => EncryptOption::Session,
        }
    }
}

/// Converts a `std::time::Duration` to an ISO 8601 duration (seconds only)
pub fn to_iso8601_duration(d: Duration) -> iso8601_duration::Duration {
    iso8601_duration::Duration::new(0.0, 0.0, 0.0, 0.0, 0.0, d.as_secs() as f32)
}

define_byte_vector!(PakePayloadVector);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakeResponse {
    /// The session task recognized by the server bound to this pake session ID
    pub task: Option<String>,

    /// PAKE response data as defined by the PAKE state incoming the request
    pub data: Option<PakePayloadVector>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone, Display)]
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
pub struct CreateKeyServiceData {
    pub curve: Curve,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreateKeyServiceDataResponse {
    pub public_key: EcPublicJwk,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeleteKeyServiceData {
    pub hsm_kid: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListKeysResponse {
    pub key_info: Vec<KeyInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignatureResponse {
    pub signature: SignatureVector,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeyInfo {
    pub created_at: Option<String>,
    pub public_key: EcPublicJwk,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListKeysRequest {
    // TODO finns någon filteringspayload i Stefans kod....
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignRequest {
    pub hsm_kid: String,
    pub message: MessageVector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PakeState {
    Evaluate,
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakeRequest {
    /// Optional authorization data required for initial PIN registrations or PIN resets
    pub authorization: Option<String>,

    pub task: Option<String>,

    #[serde(rename = "data")]
    pub data: PakePayloadVector,
}

// TODO: Move this to operations/authentication.rs?
impl PakeRequest {
    /// Creates a PakeRequest from an InnerRequest
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

#[derive(Debug, Clone)]
pub struct R2psServerConfig {
    //pub private_key_jwk: Jwk,
    pub server_public_key: Pem,
    pub server_private_key: Pem,
}

#[derive(Debug, Clone, Serialize, Deserialize, Display)]
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
    UnsupportedContext,
    InternalServerError,
    Unknown,
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

impl From<JoseError> for ServiceRequestError {
    fn from(_: JoseError) -> Self {
        ServiceRequestError::JweError
    }
}

#[derive(Debug)]
pub enum R2psRequestError {
    ConnectionError,
    UnknownClient,
    OuterJwsError,
    DecryptionError,
    EncryptionError,
    UnsupportedContext,
    NotImplemented,
    ServiceError(ServiceRequestError),
    InvalidState,
    UnknownSession,
    InnerJweError,
}
