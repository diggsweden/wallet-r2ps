// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::domain::{DeviceHsmState, EcPublicJwk};
use hsm_common::{Curve, TypedJws};
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
    /// Client JWS public key; the worker verifies device request signatures with this key
    pub client_jws_public_key: EcPublicJwk,
    /// Client JWE public key; the worker encrypts pre-session responses to this key (ECDH-ES)
    pub client_jwe_public_key: EcPublicJwk,
    /// Kafka topic the worker should send its response to
    pub response_topic: String,
    /// Curve used to generate the initial HSM key during state initialization.
    pub initial_key_curve: Curve,
}

/// Response containing the newly created device state and a one-time authorization code.
/// The state_jws contains a JWS-encoded DeviceHsmState.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StateInitResponse {
    /// Correlation ID matching the original request
    pub request_id: String,
    /// JWS-encoded device state. Opaque to API consumers; the inner type
    /// (`DeviceHsmState`) is worker-internal and not part of the public schema.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub state_jws: TypedJws<DeviceHsmState>,
    /// One-time authorization code for device registration
    pub dev_authorization_code: String,
    /// Server JWS public key. Clients use this for JWS verification only
    pub server_jws_public_key: EcPublicJwk,
    /// Server JWE public key. Clients encrypt pre-auth inner JWEs to this key (ECDH-ES).
    pub server_jwe_public_key: EcPublicJwk,
    /// OPAQUE server identifier used during registration (must match on authenticate)
    pub opaque_server_id: String,
    /// Public key of the initial HSM key generated during state initialization.
    pub initial_hsm_key: EcPublicJwk,
}
