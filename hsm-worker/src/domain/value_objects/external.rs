// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Worker-specific Kafka wire types.
//!
//! These mirror `hsm_common::{HsmWorkerRequest, HsmWorkerResponse}` but type
//! `state_jws` as `TypedJws<DeviceHsmState>` so that generated documentation
//! reflects the actual payload rather than a plain string.

use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

use crate::domain::{DeviceHsmState, OuterRequest, OuterResponse, Status};
use hsm_common::TypedJws;

/// HSM worker request received from Kafka.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerRequest {
    /// Unique identifier for this request
    pub request_id: String,
    /// JWS-encoded device state. Opaque to API consumers; the inner type
    /// (`DeviceHsmState`) is worker-internal and not part of the public schema.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub state_jws: TypedJws<DeviceHsmState>,
    /// JWS-encoded outer request envelope (OuterRequest)
    pub outer_request_jws: TypedJws<OuterRequest>,
    /// Kafka topic the worker should send its response to
    pub response_topic: String,
}

/// Worker response sent via Kafka.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HsmWorkerResponse {
    /// Correlation ID matching the original request
    pub request_id: String,
    /// JWS-encoded updated device state. Opaque to API consumers; the inner type
    /// (`DeviceHsmState`) is worker-internal and not part of the public schema.
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub state_jws: Option<TypedJws<DeviceHsmState>>,
    /// JWS-encoded service response (OuterResponse)
    pub outer_response_jws: Option<TypedJws<OuterResponse>>,
    /// The result status of the operation
    pub status: Status,
    /// Error message if the operation failed
    pub error_message: Option<String>,
}
