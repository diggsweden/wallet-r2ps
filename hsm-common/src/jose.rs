// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Shared JWS/JWE plumbing using JOSE primitives.
//!
//! All functions operate on raw bytes and `josekit::jwk::Jwk` — no domain types.
//! Callers are responsible for key management and format conversion.
//!
//! Algorithms:
//!   - JWS: ES256 (ECDSA P-256)
//!   - JWE device: ECDH-ES + A256GCM
//!   - JWE session: dir + A256GCM

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use josekit::jwe::alg::direct::DirectJweAlgorithm;
use josekit::jwe::{self, ECDH_ES, JweHeader};
use josekit::jwk::Jwk;
use josekit::jws::{self, ES256, JwsHeader};

#[derive(Debug)]
pub enum JoseError {
    Sign,
    Verify,
    Encrypt,
    Decrypt,
    InvalidKey,
}

/// Extract `kid` from the first segment (header) of a compact JWS or JWE
/// without verifying the token.
pub fn peek_kid(compact: &str) -> Option<String> {
    let header_bytes = URL_SAFE_NO_PAD
        .decode(compact.split('.').next().unwrap_or(""))
        .ok()?;
    let header: serde_json::Value = serde_json::from_slice(&header_bytes).ok()?;
    header
        .get("kid")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

// ─── JWE: ECDH-ES + A256GCM ──────────────────────────────────────────────────

/// Encrypt `plaintext` as a compact JWE using ECDH-ES + A256GCM.
/// `kid` is written into the JWE header (informational only; use `"device"` by convention).
pub fn jwe_encrypt_ecdh_es(
    plaintext: &[u8],
    public_key: &Jwk,
    kid: &str,
) -> Result<String, JoseError> {
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");
    header.set_key_id(kid);
    let encrypter = ECDH_ES
        .encrypter_from_jwk(public_key)
        .map_err(|_| JoseError::InvalidKey)?;
    jwe::serialize_compact(plaintext, &header, &encrypter).map_err(|_| JoseError::Encrypt)
}

/// Decrypt a compact ECDH-ES + A256GCM JWE.
///
/// The `kid` claim in the JWE header is ignored: `private_key`'s own kid (if any)
/// is stripped before decryption to avoid josekit rejecting a kid mismatch
/// (the server always writes `kid = "device"` in the header).
pub fn jwe_decrypt_ecdh_es(jwe_str: &str, private_key: &Jwk) -> Result<Vec<u8>, JoseError> {
    let mut jwk_map = private_key.as_ref().clone();
    jwk_map.remove("kid");
    let decryption_jwk = Jwk::from_map(jwk_map).map_err(|_| JoseError::InvalidKey)?;
    let decrypter = ECDH_ES
        .decrypter_from_jwk(&decryption_jwk)
        .map_err(|_| JoseError::InvalidKey)?;
    let (payload, _) =
        jwe::deserialize_compact(jwe_str, &decrypter).map_err(|_| JoseError::Decrypt)?;
    Ok(payload)
}

// ─── JWE: dir + A256GCM ──────────────────────────────────────────────────────

/// Encrypt `plaintext` as a compact JWE using dir + A256GCM (32-byte session key).
/// `kid` is written into the JWE header (use `"session"` by convention).
pub fn jwe_encrypt_dir(
    plaintext: &[u8],
    session_key: &[u8],
    kid: &str,
) -> Result<String, JoseError> {
    let mut header = JweHeader::new();
    header.set_algorithm("dir");
    header.set_content_encryption("A256GCM");
    header.set_key_id(kid);
    let encrypter = DirectJweAlgorithm::Dir
        .encrypter_from_bytes(session_key)
        .map_err(|_| JoseError::InvalidKey)?;
    jwe::serialize_compact(plaintext, &header, &encrypter).map_err(|_| JoseError::Encrypt)
}

/// Decrypt a compact dir + A256GCM JWE using a 32-byte session key.
pub fn jwe_decrypt_dir(jwe_str: &str, session_key: &[u8]) -> Result<Vec<u8>, JoseError> {
    let decrypter = DirectJweAlgorithm::Dir
        .decrypter_from_bytes(session_key)
        .map_err(|_| JoseError::InvalidKey)?;
    let (payload, _) =
        jwe::deserialize_compact(jwe_str, &decrypter).map_err(|_| JoseError::Decrypt)?;
    Ok(payload)
}

// ─── JWS: ES256 ──────────────────────────────────────────────────────────────

/// Sign `payload` bytes as a compact ES256 JWS.
pub fn jws_sign(payload: &[u8], private_key: &Jwk, kid: &str) -> Result<String, JoseError> {
    let mut header = JwsHeader::new();
    header.set_algorithm("ES256");
    header.set_key_id(kid);
    let signer = ES256
        .signer_from_jwk(private_key)
        .map_err(|_| JoseError::InvalidKey)?;
    jws::serialize_compact(payload, &header, &signer).map_err(|_| JoseError::Sign)
}

/// Verify an ES256 compact JWS and return the raw payload bytes.
pub fn jws_verify(jws_str: &str, public_key: &Jwk) -> Result<Vec<u8>, JoseError> {
    let verifier = ES256
        .verifier_from_jwk(public_key)
        .map_err(|_| JoseError::InvalidKey)?;
    let (payload, _) =
        jws::deserialize_compact(jws_str, &verifier).map_err(|_| JoseError::Verify)?;
    Ok(payload)
}

/// Decode the payload of a compact JWS without verifying the signature.
/// Only safe when the transport layer guarantees integrity (e.g. Kafka + TLS).
pub fn jws_decode_unverified(jws_str: &str) -> Result<Vec<u8>, JoseError> {
    let parts: Vec<&str> = jws_str.split('.').collect();
    if parts.len() != 3 {
        return Err(JoseError::Verify);
    }
    URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| JoseError::Verify)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use josekit::jwk::alg::ec::EcKeyPair;

    fn make_ec_keypair() -> (Jwk, Jwk) {
        let key_pair = EcKeyPair::generate(josekit::jwk::alg::ec::EcCurve::P256).unwrap();
        let private_jwk = key_pair.to_jwk_private_key();
        let public_jwk = key_pair.to_jwk_public_key();
        (private_jwk, public_jwk)
    }

    fn make_session_key() -> Vec<u8> {
        vec![0x42u8; 32]
    }

    #[test]
    fn test_jwe_dir_round_trip() {
        let key = make_session_key();
        let plaintext = b"hello dir world";
        let jwe = jwe_encrypt_dir(plaintext, &key, "session").unwrap();
        let decrypted = jwe_decrypt_dir(&jwe, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_jwe_ecdh_es_round_trip() {
        let (private_jwk, public_jwk) = make_ec_keypair();
        let plaintext = b"hello ecdh-es world";
        let jwe = jwe_encrypt_ecdh_es(plaintext, &public_jwk, "device").unwrap();
        let decrypted = jwe_decrypt_ecdh_es(&jwe, &private_jwk).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_jwe_ecdh_es_decrypt_ignores_kid_mismatch() {
        let (private_jwk, public_jwk) = make_ec_keypair();
        // Server always writes kid="device"; client key has no kid or a different kid
        let jwe = jwe_encrypt_ecdh_es(b"payload", &public_jwk, "device").unwrap();
        // Add a different kid to the private key — decrypt must still succeed
        let mut map = private_jwk.as_ref().clone();
        map.insert(
            "kid".to_string(),
            serde_json::Value::String("thumbprint-abc".to_string()),
        );
        let private_with_kid = Jwk::from_map(map).unwrap();
        let decrypted = jwe_decrypt_ecdh_es(&jwe, &private_with_kid).unwrap();
        assert_eq!(decrypted, b"payload");
    }

    #[test]
    fn test_jws_sign_verify_round_trip() {
        let (private_jwk, public_jwk) = make_ec_keypair();
        let payload = b"{\"hello\":\"world\"}";
        let signed = jws_sign(payload, &private_jwk, "my-kid").unwrap();
        let verified = jws_verify(&signed, &public_jwk).unwrap();
        assert_eq!(verified, payload);
    }

    #[test]
    fn test_peek_kid_jws() {
        let (private_jwk, _) = make_ec_keypair();
        let signed = jws_sign(b"data", &private_jwk, "test-kid").unwrap();
        assert_eq!(peek_kid(&signed), Some("test-kid".to_string()));
    }

    #[test]
    fn test_peek_kid_jwe() {
        let (_, public_jwk) = make_ec_keypair();
        let jwe = jwe_encrypt_ecdh_es(b"data", &public_jwk, "device").unwrap();
        assert_eq!(peek_kid(&jwe), Some("device".to_string()));
    }

    #[test]
    fn test_jws_decode_unverified() {
        let (private_jwk, _) = make_ec_keypair();
        let payload = b"{\"foo\":1}";
        let signed = jws_sign(payload, &private_jwk, "k").unwrap();
        let decoded = jws_decode_unverified(&signed).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_jws_decode_unverified_bad_format() {
        assert!(jws_decode_unverified("only.two").is_err());
    }
}
