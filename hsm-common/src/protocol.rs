// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Shared wire types for Kafka messages and protocol envelopes.

use serde::{Deserialize, Serialize};
use strum_macros::Display;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;
use uuid::Uuid;

use crate::types::{TypedJwe, TypedJws};

// ─── Internal byte-vector macro (not exported) ───────────────────────────────

macro_rules! define_byte_vector_base {
    ($(#[$attr:meta])* $name:ident) => {
        $(#[$attr])*
        #[derive(Clone)]
        pub struct $name(Vec<u8>);

        impl $name {
            pub fn new(x: Vec<u8>) -> Self { $name(x) }
            pub fn to_vec(self) -> Vec<u8> { self.0 }
        }

        impl std::ops::Deref for $name {
            type Target = Vec<u8>;
            fn deref(&self) -> &Vec<u8> { &self.0 }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] { &self.0 }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                s.serialize_str(&STANDARD.encode(&self.0))
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                let s = String::deserialize(d)?;
                STANDARD.decode(&s).map($name).map_err(serde::de::Error::custom)
            }
        }
    };
}

macro_rules! define_byte_vector {
    ($(#[$attr:meta])* $name:ident) => {
        define_byte_vector_base!($(#[$attr])* $name);

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), hex::encode(&self.0))
            }
        }
    };
}

// ─── Byte vectors ─────────────────────────────────────────────────────────────

define_byte_vector!(PakePayloadVector);
define_byte_vector!(MessageVector);
define_byte_vector!(SignatureVector);

// ─── Common ───────────────────────────────────────────────────────────────────

/// EC public key in JWK format.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EcPublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kid: String,
}

/// Unique session identifier (UUID v4).
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
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ─── Enums ───────────────────────────────────────────────────────────────────

/// Operation result status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Display)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum Status {
    #[serde(rename = "OK")]
    #[strum(serialize = "OK")]
    Ok,
    #[serde(rename = "ERROR")]
    #[strum(serialize = "ERROR")]
    Error,
}

/// Identifies the operation requested by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationId {
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

/// Which encryption layer protects the inner JWE payload.
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
        }
    }
}

/// Elliptic curve identifier for key generation.
#[derive(Debug, Clone, Serialize, Deserialize, Display)]
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

/// Current phase of the PAKE protocol exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PakeState {
    Evaluate,
    Finalize,
}

// ─── Kafka wire types ─────────────────────────────────────────────────────────

/// HSM worker request received from Kafka (wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerRequest {
    pub request_id: String,
    /// JWS-encoded DeviceHsmState (compact serialization); opaque to hsm-common
    pub state_jws: String,
    /// JWS-encoded OuterRequest (compact serialization)
    pub outer_request_jws: TypedJws<OuterRequest>,
}

/// Worker response sent via Kafka (wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerResponse {
    pub request_id: String,
    /// JWS-encoded updated DeviceHsmState (compact serialization); opaque to hsm-common
    pub state_jws: Option<String>,
    /// JWS-encoded OuterResponse (compact serialization)
    pub outer_response_jws: Option<TypedJws<OuterResponse>>,
    pub status: Status,
    pub error_message: Option<String>,
}

/// State initialisation request sent via Kafka.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StateInitRequest {
    pub request_id: String,
    pub public_key: EcPublicJwk,
}

/// State initialisation response received from Kafka.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StateInitResponse {
    pub request_id: String,
    /// JWS-encoded DeviceHsmState (compact serialization)
    pub state_jws: String,
    pub dev_authorization_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_jws_public_key: Option<EcPublicJwk>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_jws_kid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opaque_server_id: Option<String>,
}

// ─── Protocol envelope types ──────────────────────────────────────────────────

/// Outer request envelope, verified via JWS using the device's public key.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct OuterRequest {
    pub version: u32,
    pub session_id: Option<SessionId>,
    pub context: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_kid: Option<String>,
    /// JWE compact serialization of the encrypted InnerRequest
    pub inner_jwe: Option<TypedJwe<InnerRequest>>,
}

/// Decrypted inner request payload.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct InnerRequest {
    pub version: u32,
    #[serde(rename = "type")]
    pub request_type: OperationId,
    pub request_counter: u32,
    pub data: Option<String>,
}

/// Outer response envelope, signed as a JWS by the server.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct OuterResponse {
    pub version: u32,
    pub session_id: Option<SessionId>,
    /// JWE compact serialization of the encrypted InnerResponse
    pub inner_jwe: Option<TypedJwe<InnerResponse>>,
    pub status: Status,
    pub error_message: Option<String>,
}

/// Decrypted inner response payload.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct InnerResponse {
    pub version: u32,
    pub data: Option<String>,
    #[cfg_attr(
        feature = "openapi",
        schema(value_type = Option<String>, format = "duration")
    )]
    pub expires_in: Option<iso8601_duration::Duration>,
    pub status: Status,
    pub error_message: Option<String>,
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

impl InnerResponse {
    pub fn ok(data: String, expires_in: Option<iso8601_duration::Duration>) -> Self {
        Self {
            version: 1,
            data: Some(data),
            expires_in,
            status: Status::Ok,
            error_message: None,
        }
    }

    pub fn error(error_message: String) -> Self {
        Self {
            version: 1,
            data: None,
            expires_in: None,
            status: Status::Error,
            error_message: Some(error_message),
        }
    }
}

// ─── Operation data types ─────────────────────────────────────────────────────

/// PAKE operation request payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PakeRequest {
    pub authorization: Option<String>,
    pub purpose: Option<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub data: PakePayloadVector,
}

/// PAKE operation response payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PakeResponse {
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>, format = "byte"))]
    pub data: Option<PakePayloadVector>,
}

/// Request data for HsmGenerateKey.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CreateKeyServiceData {
    pub curve: Curve,
}

/// Response data from HsmGenerateKey.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CreateKeyServiceDataResponse {
    pub public_key: EcPublicJwk,
}

/// Request data for HsmDeleteKey.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeleteKeyServiceData {
    pub hsm_kid: String,
}

/// Request data for HsmListKeys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListKeysRequest {}

/// Response data from HsmListKeys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListKeysResponse {
    pub key_info: Vec<KeyInfo>,
}

/// Information about a single HSM-managed key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct KeyInfo {
    pub created_at: Option<String>,
    pub public_key: EcPublicJwk,
}

/// Request data for HsmSign.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SignRequest {
    pub hsm_kid: String,
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub message: MessageVector,
}

/// Response data from HsmSign.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct SignatureResponse {
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
    pub signature: SignatureVector,
}
