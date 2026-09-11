// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::{
    application::self_test_spi_port::{SelfTestProbe, TsfClaim},
    infrastructure::self_test_probes::crypto_es256_kat::{
        CryptoEs256KatProbe, KAT_EXPECTED_PAYLOAD, KAT_TAMPERED_JWS, KAT_VALID_JWS,
    },
};
use hsm_common::jose;

use crate::infrastructure::self_test_probes::crypto_es256_kat::kat_public_jwk;

#[test]
fn positive_vector_verifies_with_expected_payload() {
    let payload = jose::jws_verify(KAT_VALID_JWS, &kat_public_jwk())
        .expect("RFC 7515 A.3 vector must verify");
    assert_eq!(payload, KAT_EXPECTED_PAYLOAD)
}

#[test]
fn tampered_vector_is_rejected() {
    let result = jose::jws_verify(KAT_TAMPERED_JWS, &kat_public_jwk());
    assert!(result.is_err(), "Tampered signature must not verify");
}

#[test]
fn probe_passes_against_pinned_vector() {
    let result = CryptoEs256KatProbe.probe();
    assert!(result.is_ok(), "Expected Ok, got {:?}", result);
}

#[test]
fn probe_reports_the_correct_name_and_claim() {
    let probe = CryptoEs256KatProbe;
    assert_eq!(probe.name(), "crypto_es256_kat");
    assert_eq!(probe.claim(), TsfClaim::CryptographicLibraries);
}
