// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Parses server response envelopes (JWS -> OuterResponse -> JWE -> InnerResponse).

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use josekit::jwe;
use josekit::jwk::Jwk;

use super::types::{InnerResponse, OuterResponse, PakeResponse, Status};

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

/// Decode a JWS payload without verifying the signature.
/// (Server responses are trusted via transport.)
fn jws_decode_payload(jws_str: &str) -> Result<serde_json::Value> {
    let parts: Vec<&str> = jws_str.split('.').collect();
    if parts.len() != 3 {
        bail!("Invalid JWS: expected 3 parts, got {}", parts.len());
    }
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("Failed to base64url decode JWS payload")?;
    serde_json::from_slice(&payload_bytes).context("Failed to parse JWS payload JSON")
}

/// Decrypt a JWE encrypted with ECDH-ES using the device's private key.
///
/// The server's response JWE has `kid: "device"` in its header, but our device
/// key JWK has `kid: "<thumbprint>"`. josekit validates kid match, so we build
/// a JWK without kid for decryption to skip that check.
fn jwe_decrypt_device(jwe_str: &str, device_private_key: &Jwk) -> Result<Vec<u8>> {
    // Build a copy of the device key without kid so josekit won't reject the
    // server's JWE header kid ("device") as a mismatch.
    let mut jwk_map = device_private_key.as_ref().clone();
    jwk_map.remove("kid");
    let decryption_jwk = Jwk::from_map(jwk_map)?;

    let decrypter = jwe::ECDH_ES
        .decrypter_from_jwk(&decryption_jwk)
        .context("Failed to create ECDH-ES decrypter")?;
    let (payload, _) =
        jwe::deserialize_compact(jwe_str, &decrypter).context("Failed to decrypt JWE (ECDH-ES)")?;
    Ok(payload)
}

/// Decrypt a JWE encrypted with dir using a 32-byte session key.
fn jwe_decrypt_session(jwe_str: &str, session_key: &[u8]) -> Result<Vec<u8>> {
    let k_b64 = URL_SAFE_NO_PAD.encode(session_key);
    let jwk_json = serde_json::json!({ "kty": "oct", "k": k_b64 });
    let oct_jwk = Jwk::from_bytes(serde_json::to_vec(&jwk_json)?)?;

    let decrypter = jwe::Dir
        .decrypter_from_jwk(&oct_jwk)
        .context("Failed to create dir decrypter")?;
    let (payload, _) =
        jwe::deserialize_compact(jwe_str, &decrypter).context("Failed to decrypt JWE (dir)")?;
    Ok(payload)
}

/// Unwrap a PAKE response (device-encrypted).
///
/// Returns the session_id, status, and raw OPAQUE bytes from PakeResponse.data.
pub fn unwrap_pake_response(
    response_jws: &str,
    device_private_key: &Jwk,
) -> Result<PakeResponseData> {
    let outer: OuterResponse = serde_json::from_value(jws_decode_payload(response_jws)?)
        .context("Failed to parse OuterResponse")?;

    let inner_jwe = match &outer.inner_jwe {
        Some(jwe) => jwe,
        None => {
            return Ok(PakeResponseData {
                session_id: outer.session_id,
                status: Status::Error.to_string(),
                data: None,
            })
        }
    };

    let inner: InnerResponse =
        serde_json::from_slice(&jwe_decrypt_device(inner_jwe, device_private_key)?)
            .context("Failed to parse InnerResponse")?;

    if inner.status != Status::Ok {
        return Ok(PakeResponseData {
            session_id: outer.session_id,
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
        session_id: outer.session_id,
        status: inner.status.to_string(),
        data: opaque_bytes,
    })
}

/// Unwrap a session-encrypted response and return the raw InnerResponse data string.
pub fn unwrap_session_response(response_jws: &str, session_key: &[u8]) -> Result<ResponseData> {
    let outer: OuterResponse = serde_json::from_value(jws_decode_payload(response_jws)?)
        .context("Failed to parse OuterResponse")?;

    let inner_jwe = match &outer.inner_jwe {
        Some(jwe) => jwe,
        None => {
            return Ok(ResponseData {
                session_id: outer.session_id,
                status: Status::Error.to_string(),
                data: None,
            })
        }
    };

    let inner: InnerResponse =
        serde_json::from_slice(&jwe_decrypt_session(inner_jwe, session_key)?)
            .context("Failed to parse InnerResponse")?;

    Ok(ResponseData {
        session_id: outer.session_id,
        status: inner.status.to_string(),
        data: inner.data,
    })
}
