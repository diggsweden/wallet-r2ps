use crate::domain::EcPublicJwk;
use serde::{Deserialize, Serialize};

/// Request to initialize a new DeviceHsmState for a client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInitRequest {
    pub request_id: String,
    pub public_key: EcPublicJwk,
}

/// Response containing newly created state and authorization code
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInitResponse {
    pub request_id: String,
    pub state_jws: String,
    pub dev_authorization_code: String,
}
