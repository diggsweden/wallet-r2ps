// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Protocol wire types matching the hsm-worker / wallet-bff format.

use serde::{Deserialize, Serialize};

// ─── Re-exported types from hsm-common ───────────────────────────────────────

pub use hsm_common::{
    CreateKeyServiceData, CreateKeyServiceDataResponse, Curve, EcPublicJwk, InnerRequest,
    InnerResponse, MessageVector, OperationId, OuterRequest, OuterResponse, PakePayloadVector,
    PakeRequest, PakeResponse, SignRequest, Status,
};

// ─── BFF REST types ───────────────────────────────────────────────────────────

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

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BffSyncResponse {
    pub correlation_id: String,
    pub status: String,
    pub result: Option<String>,
    pub result_url: Option<String>,
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
    pub dev_authorization_code: Option<String>,
}
