// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Parses server response envelopes (JWS -> OuterResponse -> JWE -> InnerResponse).

use anyhow::{Context, Result};
use hsm_common::jose;
use josekit::jwk::Jwk;

use super::types::{InnerResponse, OuterResponse, PakeResponse, SessionId, Status};

/// Decoded payload from a PAKE response.
#[allow(dead_code)]
pub struct PakeResponseData {
    pub session_id: Option<String>,
    pub status: String,
    pub data: Option<Vec<u8>>,
}

/// Decoded payload from a device/session response.
#[allow(dead_code)]
pub struct ResponseData {
    pub session_id: Option<String>,
    pub status: String,
    pub data: Option<String>,
}

/// Unwrap a PAKE response (device-encrypted).
///
/// Returns the session_id, status, and raw OPAQUE bytes from PakeResponse.data.
pub fn unwrap_pake_response(
    response_jws: &str,
    device_private_key: &Jwk,
) -> Result<PakeResponseData> {
    let outer: OuterResponse = serde_json::from_slice(
        &jose::jws_decode_unverified(response_jws).context("JWS decode failed")?,
    )
    .context("Failed to parse OuterResponse")?;

    let inner_jwe = match &outer.inner_jwe {
        Some(jwe) => jwe,
        None => {
            return Ok(PakeResponseData {
                session_id: outer.session_id.map(SessionId::into_string),
                status: Status::Error.to_string(),
                data: None,
            })
        }
    };

    let inner: InnerResponse = serde_json::from_slice(
        &jose::jwe_decrypt_ecdh_es(inner_jwe.as_str(), device_private_key)
            .context("ECDH-ES decrypt failed")?,
    )
    .context("Failed to parse InnerResponse")?;

    if inner.status != Status::Ok {
        return Ok(PakeResponseData {
            session_id: outer.session_id.map(SessionId::into_string),
            status: inner.status.to_string(),
            data: None,
        });
    }

    let opaque_bytes = inner
        .data
        .as_ref()
        .and_then(|data_str| serde_json::from_str::<PakeResponse>(data_str).ok())
        .and_then(|pake_resp| pake_resp.data)
        .map(|pv| pv.to_vec());

    Ok(PakeResponseData {
        session_id: outer.session_id.map(SessionId::into_string),
        status: inner.status.to_string(),
        data: opaque_bytes,
    })
}

/// Unwrap a session-encrypted response and return the raw InnerResponse data string.
pub fn unwrap_session_response(response_jws: &str, session_key: &[u8]) -> Result<ResponseData> {
    let outer: OuterResponse = serde_json::from_slice(
        &jose::jws_decode_unverified(response_jws).context("JWS decode failed")?,
    )
    .context("Failed to parse OuterResponse")?;

    let inner_jwe = match &outer.inner_jwe {
        Some(jwe) => jwe,
        None => {
            return Ok(ResponseData {
                session_id: outer.session_id.map(SessionId::into_string),
                status: Status::Error.to_string(),
                data: None,
            })
        }
    };

    let inner: InnerResponse = serde_json::from_slice(
        &jose::jwe_decrypt_dir(inner_jwe.as_str(), session_key).context("dir decrypt failed")?,
    )
    .context("Failed to parse InnerResponse")?;

    Ok(ResponseData {
        session_id: outer.session_id.map(SessionId::into_string),
        status: inner.status.to_string(),
        data: inner.data,
    })
}
