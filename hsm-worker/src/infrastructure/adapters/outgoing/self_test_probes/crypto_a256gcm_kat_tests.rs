// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::self_test_spi_port::{SelfTestProbe, TsfClaim};
use crate::infrastructure::self_test_probes::crypto_a256gcm_kat::{
    CryptoA256GcmKatProbe, KAT_EXPECTED_PLAINTEXT, KAT_SESSION_KEY, KAT_TAMPERED_AAD_JWE,
    KAT_TAMPERED_TAG_JWE, KAT_VALID_JWE,
};
use hsm_common::jose;

#[test]
fn positive_vector_decrypts_to_expected_plaintext() {
    let plaintext = jose::jwe_decrypt_dir(KAT_VALID_JWE, &KAT_SESSION_KEY)
        .expect("Pinned dir+A256GCM vector must decrypt");
    assert_eq!(plaintext, KAT_EXPECTED_PLAINTEXT);
}

#[test]
fn tampered_authentication_tag_is_rejected() {
    let result = jose::jwe_decrypt_dir(KAT_TAMPERED_TAG_JWE, &KAT_SESSION_KEY);
    assert!(
        result.is_err(),
        "Tampered authentication tag must not decrypt"
    );
}

#[test]
fn tampered_protected_header_is_rejected() {
    let result = jose::jwe_decrypt_dir(KAT_TAMPERED_AAD_JWE, &KAT_SESSION_KEY);
    assert!(
        result.is_err(),
        "Tampered protection header must not decrypt"
    );
}

#[test]
fn probe_passes_against_pinned_vector() {
    let result = CryptoA256GcmKatProbe.probe();
    assert!(result.is_ok(), "Expected Ok, got {:?}", result);
}

#[test]
fn probe_reports_the_correct_name_and_claim() {
    let probe = CryptoA256GcmKatProbe;
    assert_eq!(probe.name(), "crypto_a256gcm_kat");
    assert_eq!(probe.claim(), TsfClaim::CryptographicLibraries);
}
