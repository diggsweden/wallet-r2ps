// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::jose_port::{JosePort, JweDecryptionKey};
use crate::domain::{InnerRequest, OperationId};
use josekit::jwe::ECDH_ES;
use josekit::jwe::JweHeader;
use p256::SecretKey;
use p256::pkcs8::EncodePublicKey;
use rstest::{fixture, rstest};

use super::jose_adapter::JoseAdapter;
use rand_core::OsRng;

struct JoseFixture {
    adapter: JoseAdapter,
    /// Public half of the JWE encryption key — JWEs must be encrypted to this key.
    encryption_public_pem: String,
    /// Public half of the JWS signing key — must NOT work as a JWE recipient.
    signing_public_pem: String,
}

#[fixture]
fn jose() -> JoseFixture {
    let signing_key = SecretKey::random(&mut OsRng);
    let encryption_key = SecretKey::random(&mut OsRng);
    let signing_public_pem = signing_key
        .public_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let encryption_public_pem = encryption_key
        .public_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    JoseFixture {
        adapter: JoseAdapter::new(signing_key, encryption_key).unwrap(),
        encryption_public_pem,
        signing_public_pem,
    }
}

fn make_device_jwe(public_key_pem_str: &str, inner_request: &InnerRequest) -> String {
    let payload = serde_json::to_vec(inner_request).unwrap();
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");
    header.set_key_id("device");
    let encrypter = ECDH_ES.encrypter_from_pem(public_key_pem_str).unwrap();
    josekit::jwe::serialize_compact(&payload, &header, &encrypter).unwrap()
}

#[rstest]
fn decrypt_device_jwe_happy_path(jose: JoseFixture) {
    let inner = InnerRequest {
        version: 1,
        request_type: OperationId::Info,
        data: Some("Hello, World! This is a secret message.".to_string()),
    };
    let jwe = make_device_jwe(&jose.encryption_public_pem, &inner);
    let bytes = jose
        .adapter
        .jwe_decrypt(&jwe, JweDecryptionKey::Device)
        .expect("jwe_decrypt failed");
    let decoded: InnerRequest =
        serde_json::from_slice(&bytes).expect("failed to deserialize InnerRequest");
    assert_eq!(decoded.version, 1);
    assert_eq!(
        decoded.data.unwrap(),
        "Hello, World! This is a secret message."
    );
}

#[rstest]
fn decrypt_rejects_jwe_encrypted_to_signing_key(jose: JoseFixture) {
    let inner = InnerRequest {
        version: 1,
        request_type: OperationId::Info,
        data: Some("must not decrypt".to_string()),
    };
    let jwe = make_device_jwe(&jose.signing_public_pem, &inner);
    assert!(
        jose.adapter
            .jwe_decrypt(&jwe, JweDecryptionKey::Device)
            .is_err(),
        "JWE encrypted to the JWS signing key must be rejected"
    );
}

#[rstest]
fn accepts_identical_signing_and_encryption_keys_with_warning() {
    // Backward compat: identical keys are accepted (with a log warning).
    let key = SecretKey::random(&mut OsRng);
    assert!(JoseAdapter::new(key.clone(), key).is_ok());
}

#[rstest]
#[case("only.four.parts.here")]
#[case("this.is.way.too.many.parts")]
#[case("missing_dots_in_this")]
#[case("")]
#[case("only.three.parts")]
#[case("not-base64!!")]
fn decrypt_rejects_invalid_jwe(jose: JoseFixture, #[case] invalid: &str) {
    assert!(
        jose.adapter
            .jwe_decrypt(invalid, JweDecryptionKey::Device)
            .is_err(),
        "should reject: {:?}",
        invalid
    );
}
