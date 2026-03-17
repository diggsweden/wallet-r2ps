// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Builds OuterRequest JWS envelopes matching the android-access-mechanism format.
//!
//! Envelope layers (innermost to outermost):
//!   payload JSON -> InnerRequest JSON -> JWE -> OuterRequest JSON -> JWS

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use josekit::jwe::{self, JweHeader};
use josekit::jwk::Jwk;
use josekit::jws::{self, JwsHeader};

use super::types::{InnerRequest, OuterRequest, PakeRequest};

/// Build a full OuterRequest JWS for a PAKE operation (registration, login).
///
/// The inner request is encrypted with ECDH-ES + A256GCM using the server's public key.
pub fn build_pake_request_jws(
    operation_type: &str,
    pake_request: &PakeRequest,
    session_id: Option<&str>,
    request_counter: u32,
    server_public_key: &Jwk,
    device_private_key: &Jwk,
    kid: &str,
) -> Result<String> {
    let inner_request = InnerRequest {
        version: 1,
        operation_type: operation_type.to_string(),
        request_counter,
        data: serde_json::to_string(pake_request)?,
    };

    let inner_bytes = serde_json::to_vec(&inner_request)?;
    let inner_jwe = jwe_encrypt_device(&inner_bytes, server_public_key)?;

    let outer_request = OuterRequest {
        version: 1,
        session_id: session_id.map(String::from),
        context: "hsm".to_string(),
        inner_jwe,
    };

    jws_sign(&outer_request, device_private_key, kid)
}

/// Build a full OuterRequest JWS for a session-encrypted operation (HSM sign, etc.).
///
/// The inner request is encrypted with dir + A256GCM using the 32-byte session key.
pub fn build_session_request_jws(
    operation_type: &str,
    data_payload: &serde_json::Value,
    session_id: &str,
    request_counter: u32,
    session_key: &[u8],
    device_private_key: &Jwk,
    kid: &str,
) -> Result<String> {
    let inner_request = InnerRequest {
        version: 1,
        operation_type: operation_type.to_string(),
        request_counter,
        data: serde_json::to_string(data_payload)?,
    };

    let inner_bytes = serde_json::to_vec(&inner_request)?;
    let inner_jwe = jwe_encrypt_session(&inner_bytes, session_key)?;

    let outer_request = OuterRequest {
        version: 1,
        session_id: Some(session_id.to_string()),
        context: "hsm".to_string(),
        inner_jwe,
    };

    jws_sign(&outer_request, device_private_key, kid)
}

// ─── JWS ───

/// Sign a JSON payload as a compact JWS (ES256).
fn jws_sign<T: serde::Serialize>(payload: &T, private_key: &Jwk, kid: &str) -> Result<String> {
    let mut header = JwsHeader::new();
    header.set_algorithm("ES256");
    header.set_key_id(kid);

    let payload_bytes = serde_json::to_vec(payload)?;

    let signer = jws::ES256
        .signer_from_jwk(private_key)
        .context("Failed to create JWS signer")?;

    let jws =
        jws::serialize_compact(&payload_bytes, &header, &signer).context("Failed to sign JWS")?;

    Ok(jws)
}

// ─── JWE: ECDH-ES (device encryption) ───

/// Encrypt plaintext bytes as JWE using ECDH-ES + A256GCM with the server's public key.
fn jwe_encrypt_device(plaintext: &[u8], server_public_key: &Jwk) -> Result<String> {
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");
    header.set_key_id("device");

    let encrypter = jwe::ECDH_ES
        .encrypter_from_jwk(server_public_key)
        .context("Failed to create ECDH-ES encrypter")?;

    let jwe = jwe::serialize_compact(plaintext, &header, &encrypter)
        .context("Failed to encrypt JWE (ECDH-ES)")?;

    Ok(jwe)
}

// ─── JWE: dir (session encryption) ───

/// Encrypt plaintext bytes as JWE using dir + A256GCM with a 32-byte session key.
fn jwe_encrypt_session(plaintext: &[u8], session_key: &[u8]) -> Result<String> {
    let mut header = JweHeader::new();
    header.set_algorithm("dir");
    header.set_content_encryption("A256GCM");
    header.set_key_id("session");

    // Build an oct JWK from the raw session key
    let k_b64 = URL_SAFE_NO_PAD.encode(session_key);
    let jwk_json = serde_json::json!({
        "kty": "oct",
        "k": k_b64
    });
    let oct_jwk = Jwk::from_bytes(serde_json::to_vec(&jwk_json)?)?;

    let encrypter = jwe::Dir
        .encrypter_from_jwk(&oct_jwk)
        .context("Failed to create dir encrypter")?;

    let jwe = jwe::serialize_compact(plaintext, &header, &encrypter)
        .context("Failed to encrypt JWE (dir)")?;

    Ok(jwe)
}
