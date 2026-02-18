use crate::application::service::operations::hsm::MessageVector;
use crate::application::service::operations::hsm::SignatureVector;
use crate::define_byte_vector;
use crate::domain::{DeviceHsmState, EcPublicJwk};
use base64::DecodeError;
use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use josekit::JoseError;
use josekit::jws::ES256;
use josekit::jws::alg::ecdsa::EcdsaJwsSigner;
use josekit::jwt::{self, JwtPayload};
use pem::Pem;
use serde::{Deserialize, Serialize};
use std::string::FromUtf8Error;
use std::time::Duration;
use strum_macros::Display;
use tracing::{debug, error, warn};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;
use uuid::Uuid;

use super::typed_jwe::TypedJwe;
use super::typed_jws::TypedJws;

/// A unique session identifier (UUID v4) used to track an active R2PS session.
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
    /// Operation completed successfully
    #[serde(rename = "OK")]
    Ok,
    /// Operation failed
    #[serde(rename = "ERROR")]
    Error,
}

/// DTO for HSM worker requests received from Kafka.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerRequestDto {
    /// Unique identifier for this request (used for correlation)
    pub request_id: String,
    /// JWS-encoded device state (DeviceHsmState)
    pub state_jws: TypedJws<DeviceHsmState>,
    /// JWS-encoded outer request envelope (OuterRequest)
    pub outer_request_jws: TypedJws<OuterRequest>,
}

/// DTO for R2PS responses sent back via Kafka.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct R2psResponseDto {
    /// Correlation ID matching the original request
    pub request_id: String,
    pub http_status: u16,
    /// JWS-encoded updated device state (DeviceHsmState)
    pub state_jws: TypedJws<DeviceHsmState>,
    /// JWS-encoded service response (OuterResponse)
    pub service_response_jws: TypedJws<OuterResponse>,
}

/// An HSM worker request containing the device state and the client's outer request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerRequest {
    /// Unique identifier for this request
    pub request_id: String,
    /// JWS-encoded device state (DeviceHsmState)
    pub state_jws: TypedJws<DeviceHsmState>,
    /// JWS-encoded outer request envelope (OuterRequest)
    pub outer_request_jws: TypedJws<OuterRequest>,
}

/// The worker's response containing updated state and the service response,
/// sent back via Kafka as a JWS-encoded message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponseJws {
    /// Correlation ID matching the original request
    pub request_id: String,
    pub http_status: u16,
    /// JWS-encoded updated device state (DeviceHsmState)
    pub state_jws: TypedJws<DeviceHsmState>,
    /// JWS-encoded service response (OuterResponse)
    pub service_response_jws: TypedJws<OuterResponse>,
}

/// The outer request envelope, verified via JWS using the device's public key.
/// Contains the protocol version, session binding, and an encrypted inner payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct OuterRequest {
    /// Protocol version
    pub version: u32,
    /// Session identifier (absent for initial requests like registration/authentication start)
    pub session_id: Option<SessionId>,
    /// Request context, currently always "hsm"
    pub context: String,
    /// JWE-encrypted inner request payload (JWE compact serialization)
    pub inner_jwe: Option<TypedJwe<InnerRequest>>,
}

impl OuterRequest {
    pub fn from_jws(
        jws: &str,
        client_public_key: &josekit::jwk::Jwk,
    ) -> Result<Self, ServiceRequestError> {
        // Create verifier from JWK using ES256 algorithm
        let verifier = ES256.verifier_from_jwk(client_public_key).map_err(|e| {
            error!("Failed to create verifier from JWK: {:?}", e);
            ServiceRequestError::InvalidClientPublicKey
        })?;

        // Decode and verify JWT
        let (payload, _header) = jwt::decode_with_verifier(jws, &verifier).map_err(|e| {
            error!("JWS verification failed: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        // Deserialize payload to OuterRequest
        let outer_request: OuterRequest =
            serde_json::from_str(&payload.to_string()).map_err(|e| {
                error!("Failed to deserialize outer request: {:?}", e);
                ServiceRequestError::JwsError
            })?;

        debug!("decoded outer request JWS: {:#?}", outer_request);
        Ok(outer_request)
    }

    pub fn peek_kid(jws: &str) -> Result<Option<String>, ServiceRequestError> {
        // Split JWS compact serialization (header.payload.signature)
        let parts: Vec<&str> = jws.split('.').collect();
        if parts.len() < 3 {
            return Err(ServiceRequestError::JwsError);
        }

        // Decode the header (first part)
        let header_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|_| ServiceRequestError::JwsError)?;

        let header: serde_json::Value =
            serde_json::from_slice(&header_bytes).map_err(|_| ServiceRequestError::JwsError)?;

        Ok(header
            .get("kid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }
}

/// The decrypted inner request payload, containing the operation type and request-specific data.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct InnerRequest {
    /// Protocol version
    pub version: u32,
    /// The operation to perform
    #[serde(rename = "type")]
    pub request_type: OperationId,
    /// Monotonically increasing counter for replay protection
    pub request_counter: u32,
    /// Operation-specific request data (serialized JSON)
    pub data: Option<String>,
}

/// The inner response payload returned to the client after decryption.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct InnerResponse {
    /// Protocol version
    pub version: u32,
    /// Operation-specific response data (serialized JSON)
    pub data: Option<String>,
    /// Time until the session expires, as an ISO 8601 duration (e.g. "PT300S")
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>, format = "duration"))]
    pub expires_in: Option<iso8601_duration::Duration>,
    /// The result status of the operation
    pub status: Status,
}

/// The outer response envelope, signed as a JWS by the server.
/// Contains the session binding and an encrypted inner response payload.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct OuterResponse {
    /// Protocol version
    pub version: u32,
    /// Session identifier
    pub session_id: Option<SessionId>,
    /// JWE-encrypted inner response payload (JWE compact serialization)
    pub inner_jwe: Option<TypedJwe<InnerResponse>>,
}

impl OuterResponse {
    pub fn to_jws(
        &self,
        signer: &EcdsaJwsSigner,
    ) -> Result<TypedJws<OuterResponse>, ServiceRequestError> {
        debug!("Outer response before JWS encoding: {:#?}", self);

        // Create JWT payload from outer_response
        let value = serde_json::to_value(self).map_err(|e| {
            error!("Failed to serialize outer response: {:?}", e);
            ServiceRequestError::SerializeResponseError
        })?;

        let map = value.as_object().cloned().ok_or_else(|| {
            error!("Failed to convert outer response to JSON object");
            ServiceRequestError::SerializeResponseError
        })?;

        let payload = JwtPayload::from_map(map).map_err(|e| {
            error!("Failed to create JwtPayload: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        // Create JWS header
        let header = josekit::jws::JwsHeader::new();

        let token = jwt::encode_with_signer(&payload, &header, signer).map_err(|e| {
            error!("Failed to encode outer response JWS: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        Ok(TypedJws::new(token))
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
/// Determines how the inner request data is interpreted and which service handles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationId {
    /// Begin OPAQUE authentication (PAKE evaluate phase)
    AuthenticateStart,
    /// Complete OPAQUE authentication (PAKE finalize phase)
    AuthenticateFinish,
    /// Begin OPAQUE registration (PAKE evaluate phase)
    RegisterStart,
    /// Complete OPAQUE registration (PAKE finalize phase)
    RegisterFinish,
    /// Change the device PIN (re-register OPAQUE credential)
    PinChange,
    /// Sign data using an HSM-managed key
    HsmSign,
    /// Perform ECDH key agreement using an HSM-managed key
    HsmEcdh,
    /// Generate a new key pair in the HSM
    HsmGenerateKey,
    /// Delete an HSM-managed key
    HsmDeleteKey,
    /// List all HSM-managed keys for this device
    HsmListKeys,
    /// End the current session
    EndSession,
    /// Store data (reserved)
    Store,
    /// Retrieve stored data (reserved)
    Retrieve,
    /// Write a log entry
    Log,
    /// Retrieve log entries
    GetLog,
    /// Retrieve server/worker information
    Info,
}

/// Specifies which encryption layer protects the inner JWE payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum EncryptOption {
    /// Encrypted with the session key (AES-256-GCM, dir)
    Session,
    /// Encrypted with the device's public key (ECDH-ES)
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

/// Response from a PAKE (Password-Authenticated Key Exchange) operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PakeResponse {
    /// The session task recognized by the server bound to this PAKE session
    pub task: Option<String>,

    /// PAKE response data (base64-encoded) as defined by the PAKE protocol state
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>, format = "byte"))]
    pub data: Option<PakePayloadVector>,
}

/// Elliptic curve identifier for key generation.
#[derive(Serialize, Deserialize, Debug, Clone, Display)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum Curve {
    /// NIST P-256 (secp256r1)
    #[serde(rename = "P-256")]
    #[strum(serialize = "P-256")]
    P256,
    /// NIST P-384 (secp384r1)
    #[serde(rename = "P-384")]
    #[strum(serialize = "P-384")]
    P384,
    /// NIST P-521 (secp521r1)
    #[serde(rename = "P-521")]
    #[strum(serialize = "P-521")]
    P521,
}
/// Request data for the HsmGenerateKey operation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CreateKeyServiceData {
    /// The elliptic curve to use for key generation
    pub curve: Curve,
}

/// Response data from the HsmGenerateKey operation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CreateKeyServiceDataResponse {
    /// The public key of the newly generated key pair
    pub public_key: EcPublicJwk,
}

/// Request data for the HsmDeleteKey operation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeleteKeyServiceData {
    /// Key identifier of the HSM key to delete
    pub hsm_kid: String,
}

/// Response data from the HsmListKeys operation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListKeysResponse {
    /// List of keys managed by the HSM for this device
    pub key_info: Vec<KeyInfo>,
}

/// Response data from the HsmSign operation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SignatureResponse {
    /// The computed signature (base64-encoded)
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub signature: SignatureVector,
}

/// Information about a single HSM-managed key.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct KeyInfo {
    /// Timestamp when this key was created (ISO 8601)
    pub created_at: Option<String>,
    /// The public key in EC JWK format
    pub public_key: EcPublicJwk,
}

/// Request data for the HsmListKeys operation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListKeysRequest {}

/// Request data for the HsmSign operation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SignRequest {
    /// Key identifier of the HSM key to sign with
    pub hsm_kid: String,
    /// The message to sign (base64-encoded)
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub message: MessageVector,
}

/// The current phase of the PAKE protocol exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PakeState {
    /// Server evaluates the client's PAKE message
    Evaluate,
    /// Server finalizes the PAKE exchange
    Finalize,
}

/// Request payload for PAKE (Password-Authenticated Key Exchange) operations
/// (RegisterStart, RegisterFinish, AuthenticateStart, AuthenticateFinish, PinChange).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PakeRequest {
    /// Optional authorization data required for initial PIN registrations or PIN resets
    pub authorization: Option<String>,

    /// The session task identifier
    pub task: Option<String>,

    /// PAKE protocol message data (base64-encoded)
    #[serde(rename = "data")]
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
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
pub struct WorkerServerConfig {
    //pub private_key_jwk: Jwk,
    pub server_public_key: Pem,
    pub server_private_key: Pem,
}

/// Errors that can occur during service request processing.
#[derive(Debug, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum ServiceRequestError {
    /// JWS signature verification or encoding failed
    JwsError,
    /// JWE encryption or decryption failed
    JweError,
    /// The PAKE request payload is malformed
    InvalidPakeRequest,
    /// The registration request is invalid
    InvalidRegistrationRequest,
    /// OPAQUE server registration start failed
    ServerRegistrationStartFailed,
    /// OPAQUE server login start failed
    ServerLoginStartFailed,
    /// OPAQUE server login finish failed
    ServerLoginFinishFailed,
    /// Failed to serialize the response
    SerializeResponseError,
    /// Failed to serialize the device state
    SerializeStateError,
    /// The service request format is invalid
    InvalidServiceRequestFormat,
    /// The stored password file could not be deserialized
    InvalidSerializedPasswordFile,
    /// The authentication request is invalid
    InvalidAuthenticateRequest,
    /// The referenced key was not found
    UnknownKey,
    /// The session does not exist or has expired
    UnknownSession,
    /// The client/device is not recognized
    UnknownClient,
    /// The client's public key is invalid
    InvalidClientPublicKey,
    /// A public key parameter is invalid
    InvalidPublicKey,
    /// A key with the same identifier already exists
    DuplicateKey,
    /// The referenced HSM key was not found
    HsmKeyNotFound,
    /// The request context is not supported
    UnsupportedContext,
    /// An internal server error occurred
    InternalServerError,
    InvalidAuthorizationCode,
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

/// Higher-level errors that can occur when processing a worker request.
#[derive(Debug)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum WorkerRequestError {
    /// Failed to connect to a required service
    ConnectionError,
    /// The client/device is not recognized
    UnknownClient,
    /// The outer JWS could not be verified
    OuterJwsError,
    /// Decryption of the inner JWE failed
    DecryptionError,
    /// Encryption of the response failed
    EncryptionError,
    /// The request context is not supported
    UnsupportedContext,
    /// The requested operation is not implemented
    NotImplemented,
    /// A service-level error occurred during operation execution
    ServiceError(ServiceRequestError),
    /// The device state is invalid or corrupted
    InvalidState,
    /// The session does not exist or has expired
    UnknownSession,
    /// The inner JWE is malformed
    InnerJweError,
}
