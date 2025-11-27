use std::time::Duration;
use josekit::jwk::Jwk;
use pem::Pem;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct R2psRequest {
    pub request_id: String,
    pub wallet_id: String,
    pub device_id: String,
    pub payload: String,
}

// Define your output message structure
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct R2PsResponse {
    pub request_id: String,
    pub wallet_id: String,
    pub device_id: String,
    pub http_status: u16,
    pub payload: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServiceRequest {
    pub client_id: String,
    pub kid: String,
    pub context: String,
    #[serde(rename = "type")]
    pub service_type: ServiceTypeId,
    pub pake_session_id: Option<String>,
    #[serde(rename = "ver")]
    pub version: Option<String>,
    pub nonce: Option<String>,
    pub iat: Option<i64>,
    pub enc: Option<String>,
    #[serde(rename = "data")]
    pub service_data: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTypeId {
    Authenticate,
    PinRegistration,
    PinChange,
    HsmEcdsa,
    HsmEcdh,
    #[serde(rename = "hsm_ec_keygen")]
    HsmEcKeygen,
    #[serde(rename = "hsm_ec_delete_key")]
    HsmEcDeleteKey,
    HsmListKeys,
    SessionEnd,
    SessionContextEnd,
    Store,
    Retrieve,
    Log,
    GetLog,
    Info,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakeResponsePayload {
    /// The PAKE session ID assigned by the server
    #[serde(rename = "pake_session_id")]
    pub pake_session_id: Option<String>,

    /// The session task recognized by the server bound to this pake session ID
    #[serde(rename = "task")]
    pub task: Option<String>,

    /// PAKE response data as defined by the PAKE state incoming the request
    #[serde(rename = "resp")]
    pub response_data: Option<String>,

    #[serde(rename = "msg")]
    pub message: Option<String>,

    #[serde(rename = "session_expiration_time")]
    pub session_expiration_time: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub ver: String,
    pub nonce: String,
    pub iat: i64,
    pub enc: String,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PakeProtocol {
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PakeState {
    Evaluate,
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakeRequestPayload {
    /// Identifies the PAKE protocol
    #[serde(rename = "protocol")]
    pub protocol: PakeProtocol,

    /// Identifies the PAKE state which determines the data content.
    /// E.g., evaluate or finalize for OPAQUE
    #[serde(rename = "state")]
    pub state: PakeState,

    /// Optional authorization data required for initial PIN registrations or PIN resets
    #[serde(rename = "authorization", skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,

    #[serde(rename = "task", skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,

    #[serde(
        rename = "session_duration",
        skip_serializing_if = "Option::is_none",
        with = "duration_serde",
        default
    )]
    pub session_duration: Option<Duration>,

    /// The PAKE request data as defined by the PAKE state
    #[serde(rename = "req")]
    pub request_data: String,
}

impl PakeRequestPayload {
    /// Serializes the payload to bytes
    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserializes the payload from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

// Helper module for Duration serialization/deserialization
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match duration {
            Some(d) => serializer.serialize_u64(d.as_secs()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = Option::<u64>::deserialize(deserializer)?;
        Ok(secs.map(Duration::from_secs))
    }
}
#[derive(Debug, Clone)]
pub struct R2psServerConfig {
    //pub private_key_jwk: Jwk,
    pub server_public_key: Pem,
    pub server_private_key: Pem,
}