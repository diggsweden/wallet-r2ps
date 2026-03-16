use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request from BFF to the REST API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "clientId": "d290f1ee-6c54-4b01-90e6-d701748f0851",
    "outerRequestJws": "eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
}))]
pub struct BffRequest {
    /// Device/wallet identifier
    pub client_id: String,

    /// JWS-signed service request envelope
    pub outer_request_jws: String,
}

/// Request to initialize new device state.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "publicKey": {
        "kty": "EC",
        "crv": "P-256",
        "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
        "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0",
        "kid": "device-key-1"
    },
    "ttl": "P30D"
}))]
pub struct NewStateRequestDto {
    /// EC public key in JWK format
    pub public_key: EcPublicJwk,

    /// Optional client ID (dev-only, should be removed in production)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Dev-only flag to overwrite existing state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,

    /// TTL in ISO 8601 duration format (e.g., "P30D", "PT1H")
    pub ttl: String,
}

/// EC Public Key in JWK format.
///
/// The `kid` field is required and identifies this key within the device's
/// key set (RFC 7517). The HSM worker uses it to look up the correct
/// public key for JWS signature verification.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "kty": "EC",
    "crv": "P-256",
    "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
    "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0",
    "kid": "device-key-1"
}))]
pub struct EcPublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    /// Key identifier (RFC 7517) — required, must not be empty
    pub kid: String,
}
