// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! FPT_TST.1 Application Note 30 — "correct operation of ... cryptographic libraries".
//!
//! Known-answer test for `dir` + A256GCM content decryption, the algorithm protecting
//! every post-authentication inner JWE. Runs through the unmodified
//! `hsm_common::jose::jwe_decrypt_dir`.
//!
//! No published `dir`+A256GCM compact-JWE vector exists — RFC 7520 §5.6's `dir` example is
//! A128GCM, and the A256GCM examples (RFC 7516 §A.1, RFC 7520 §5.2) wrap the CEK with
//! RSA-OAEP. This vector is therefore built from RFC 7516 §A.1's published CEK, IV and
//! plaintext under a `dir` protected header, with the authentication tag computed by an
//! implementation independent of this codebase (Python `cryptography`). Because AES-GCM's
//! CTR keystream depends only on key and IV, the ciphertext below is byte-for-byte the
//! ciphertext RFC 7516 §A.1.6 publishes; only the 16-byte tag differs, the AAD having
//! changed with the header.
//!
//! Two negative cases, not one. An AEAD fails open by returning plaintext without
//! checking the tag, and the two authenticated regions fail independently: a tampered tag
//! catches a missing GHASH comparison, and a tampered protected header catches a tag
//! computed over the wrong authenticated data — a bug the tag case passes straight over.

use hsm_common::jose;

use crate::application::self_test_spi_port::{SelfTestError, SelfTestProbe, TsfClaim};

// RFC 7516 §A.1.2 — the 256-bit Content Encryption Key, verbatim.
pub(crate) const KAT_SESSION_KEY: [u8; 32] = [
    177, 161, 244, 128, 84, 143, 225, 115, 63, 180, 3, 255, 107, 154, 212, 246, 138, 7, 110, 91,
    112, 46, 34, 105, 47, 130, 203, 46, 122, 234, 64, 252,
];

// Protected header {"alg":"dir","enc":"A256GCM"} (RFC 7516 §A.1.1's header with only the
// alg value swapped), the JWE Encrypted Key empty as `dir` requires, IV from §A.1.4 and
// ciphertext from §A.1.6 unchanged, tag recomputed for the new AAD.
pub(crate) const KAT_VALID_JWE: &str = "eyJhbGciOiJkaXIiLCJlbmMiOiJBMjU2R0NNIn0..48V1_ALb6US04U3b.5eym8TW_c8SuK0ltJ3rpYIzOeDQz7TALvtu6UG9oMo4vpzs9tX_EFShS8iB7j6jiSdiwkIr3ajwQzaBtQD_A.Q_HtEPSRG-ujQXQqWM1jlg";

// Low bit of the tag's last byte flipped (g -> w). Header, IV and ciphertext identical to
// the valid JWE. Must be rejected.
pub(crate) const KAT_TAMPERED_TAG_JWE: &str = "eyJhbGciOiJkaXIiLCJlbmMiOiJBMjU2R0NNIn0..48V1_ALb6US04U3b.5eym8TW_c8SuK0ltJ3rpYIzOeDQz7TALvtu6UG9oMo4vpzs9tX_EFShS8iB7j6jiSdiwkIr3ajwQzaBtQD_A.Q_HtEPSRG-ujQXQqWM1jlw";

// Protected header extended with `"kid":"kat"`; IV, ciphertext and tag identical to the
// valid JWE. The header is the AAD, so this must be rejected on the tag comparison.
pub(crate) const KAT_TAMPERED_AAD_JWE: &str = "eyJhbGciOiJkaXIiLCJlbmMiOiJBMjU2R0NNIiwia2lkIjoia2F0In0..48V1_ALb6US04U3b.5eym8TW_c8SuK0ltJ3rpYIzOeDQz7TALvtu6UG9oMo4vpzs9tX_EFShS8iB7j6jiSdiwkIr3ajwQzaBtQD_A.Q_HtEPSRG-ujQXQqWM1jlg";

// RFC 7516 §A.1 — the plaintext, exactly.
pub(crate) const KAT_EXPECTED_PLAINTEXT: &[u8] =
    b"The true sign of intelligence is not knowledge but imagination.";

pub struct CryptoA256GcmKatProbe;

impl SelfTestProbe for CryptoA256GcmKatProbe {
    fn name(&self) -> &'static str {
        "crypto_a256gcm_kat"
    }

    fn claim(&self) -> TsfClaim {
        TsfClaim::CryptographicLibraries
    }

    fn probe(&self) -> Result<(), SelfTestError> {
        let plaintext =
            jose::jwe_decrypt_dir(KAT_VALID_JWE, &KAT_SESSION_KEY).map_err(|_| SelfTestError {
                detail: "A256GCM KAT: pinned positive vector failed to decrypt".to_string(),
            })?;
        if plaintext != KAT_EXPECTED_PLAINTEXT {
            return Err(SelfTestError {
                detail: "A256GCM KAT: positive vector decrypted but plaintext did not match"
                    .to_string(),
            });
        }

        if jose::jwe_decrypt_dir(KAT_TAMPERED_TAG_JWE, &KAT_SESSION_KEY).is_ok() {
            return Err(SelfTestError {
                detail: "A256GCM KAT: tampered authentication tag was incorrectly accepted"
                    .to_string(),
            });
        }

        if jose::jwe_decrypt_dir(KAT_TAMPERED_AAD_JWE, &KAT_SESSION_KEY).is_ok() {
            return Err(SelfTestError {
                detail: "A256GCM KAT: tampered protected header was incorrectly accepted"
                    .to_string(),
            });
        }

        Ok(())
    }
}
