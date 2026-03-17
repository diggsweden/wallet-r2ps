// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Protocol wire types matching the rust-r2ps-worker / wallet-bff-ws format.
//!
//! Response types have fields that may not be directly read in code but are
//! needed for correct JSON deserialization.

use serde::{Deserialize, Serialize};

// ─── Request types ───

#[derive(Serialize)]
pub struct OuterRequest {
    pub version: u32,
    pub session_id: Option<String>,
    pub context: String,
    pub inner_jwe: String,
}

#[derive(Serialize)]
pub struct InnerRequest {
    pub version: u32,
    #[serde(rename = "type")]
    pub operation_type: String,
    pub request_counter: u32,
    pub data: String, // JSON-stringified payload (double-serialized)
}

#[derive(Serialize)]
pub struct PakeRequest {
    pub authorization: Option<String>,
    pub task: Option<String>,
    pub data: String, // base64-standard encoded OPAQUE bytes
}

#[derive(Serialize)]
pub struct SignRequestPayload {
    pub hsm_kid: String,
    pub message: String, // base64-standard encoded hash bytes
}

#[derive(Serialize)]
pub struct GenerateKeyPayload {
    pub curve: String,
}

// ─── Response types ───

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct OuterResponse {
    pub version: u32,
    pub session_id: Option<String>,
    pub inner_jwe: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct InnerResponse {
    pub version: u32,
    pub data: Option<String>,
    pub expires_in: Option<serde_json::Value>,
    pub status: String,
    pub hsm_state_version: Option<i64>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct PakeResponse {
    pub task: Option<String>,
    pub data: Option<String>,
}

// ─── BFF REST types ───

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffRequest {
    pub client_id: String,
    pub outer_request_jws: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BffNewStateRequest {
    pub public_key: EcPublicJwk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EcPublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    pub kid: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BffSyncResponse {
    pub correlation_id: String,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<BffErrorDetail>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BffErrorDetail {
    pub message: String,
    pub http_status: u16,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BffNewStateResponse {
    pub status: String,
    pub client_id: String,
    pub service_response_jws: String,
}

/// Parsed response from unwrapping a device-encrypted init response.
#[derive(Deserialize, Debug)]
pub struct DeviceInitData {
    pub dev_authorization_code: Option<String>,
}

/// Parsed response from HSM generate key.
#[derive(Deserialize, Debug)]
pub struct HsmGenerateKeyResponse {
    pub public_key: Option<HsmPublicKey>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct HsmPublicKey {
    pub kty: Option<String>,
    pub crv: Option<String>,
    pub x: Option<String>,
    pub y: Option<String>,
    pub kid: Option<String>,
}
