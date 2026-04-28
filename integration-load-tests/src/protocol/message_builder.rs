// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Builds OuterRequest JWS envelopes matching the android-access-mechanism format.
//!
//! Envelope layers (innermost to outermost):
//!   payload JSON -> InnerRequest JSON -> JWE -> OuterRequest JSON -> JWS

use anyhow::{Context, Result};
use hsm_common::jose;
use josekit::jwk::Jwk;

use super::types::{InnerRequest, OperationId, OuterRequest, PakeRequest, SessionId, TypedJwe};

/// Build a full OuterRequest JWS for a PAKE operation (registration, login).
///
/// The inner request is encrypted with ECDH-ES + A256GCM using the server's public key.
pub fn build_pake_request_jws(
    operation_type: OperationId,
    pake_request: &PakeRequest,
    session_id: Option<&str>,
    server_public_key: &Jwk,
    device_private_key: &Jwk,
    kid: &str,
) -> Result<String> {
    let inner_request = InnerRequest {
        version: 1,
        request_type: operation_type,
        data: Some(serde_json::to_string(pake_request)?),
    };
    let inner_bytes = serde_json::to_vec(&inner_request)?;
    let inner_jwe = jose::jwe_encrypt_ecdh_es(&inner_bytes, server_public_key, "device")
        .context("ECDH-ES encrypt failed")?;
    let outer_request = OuterRequest {
        version: 1,
        session_id: session_id.map(|s| SessionId::from(s.to_string())),
        context: "hsm".to_string(),
        server_kid: None,
        inner_jwe: Some(TypedJwe::new(inner_jwe)),
        nonce: uuid::Uuid::new_v4().to_string(),
    };
    jose::jws_sign(
        &serde_json::to_vec(&outer_request)?,
        device_private_key,
        kid,
    )
    .context("JWS sign failed")
}

/// Build a full OuterRequest JWS for a session-encrypted operation (HSM sign, etc.).
///
/// The inner request is encrypted with dir + A256GCM using the 32-byte session key.
pub fn build_session_request_jws(
    operation_type: OperationId,
    data_payload: &serde_json::Value,
    session_id: &str,
    session_key: &[u8],
    device_private_key: &Jwk,
    kid: &str,
) -> Result<String> {
    let inner_request = InnerRequest {
        version: 1,
        request_type: operation_type,
        data: Some(serde_json::to_string(data_payload)?),
    };
    let inner_bytes = serde_json::to_vec(&inner_request)?;
    let inner_jwe = jose::jwe_encrypt_dir(&inner_bytes, session_key, "session")
        .context("dir encrypt failed")?;
    let outer_request = OuterRequest {
        version: 1,
        session_id: Some(SessionId::from(session_id.to_string())),
        context: "hsm".to_string(),
        server_kid: None,
        inner_jwe: Some(TypedJwe::new(inner_jwe)),
        nonce: uuid::Uuid::new_v4().to_string(),
    };
    jose::jws_sign(
        &serde_json::to_vec(&outer_request)?,
        device_private_key,
        kid,
    )
    .context("JWS sign failed")
}
