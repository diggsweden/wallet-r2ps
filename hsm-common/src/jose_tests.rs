// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::jose::*;
use josekit::jwk::Jwk;
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
