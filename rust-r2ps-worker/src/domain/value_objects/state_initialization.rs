use crate::domain::value_objects::typed_jws::TypedJws;
use crate::domain::{DeviceHsmState, EcPublicJwk};
use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Request to initialize a new DeviceHsmState for a client.
/// Triggers creation of a fresh device state with the provided public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StateInitRequest {
    /// Correlation ID for this initialization request
    pub request_id: String,
    pub public_key: EcPublicJwk,
}

/// Response containing the newly created device state and a one-time authorization code.
/// The state_jws contains a JWS-encoded DeviceHsmState.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StateInitResponse {
    /// Correlation ID matching the original request
    pub request_id: String,
    pub state_jws: TypedJws<DeviceHsmState>,
    /// One-time authorization code for device registration
    pub dev_authorization_code: String,
    /// Server JWS public key (EC JWK); clients may use this for JWE encryption and JWS verification
    pub server_jws_public_key: EcPublicJwk,
    /// KID of the server JWS public key
    pub server_jws_kid: String,
    /// OPAQUE server identifier used during registration (must match on authenticate)
    pub opaque_server_id: String,
}
