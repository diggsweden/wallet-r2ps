// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! FPT_TST.1 Application Note 30 — "correct operation of ... cryptographic libraries".
//!
//! Known-answer test against RFC 7515 §A.3 ("Example JWS Using ECDSA P-256 SHA-256"),
//! the JWS specification's own worked example — a published vector independent of this
//! codebase, already in compact-JWS form so it runs through the unmodified
//! `hsm_common::jose::jws_verify`. Verifies a genuine signature and, just as importantly,
//! rejects a tampered one (same header/payload, one bit flipped in the signature) — a
//! verifier stubbed to always return success would pass the first assertion and fail the
//! second.

use hsm_common::jose as hsmjose;
use josekit::jwk::Jwk;

use crate::application::self_test_spi_port::{SelfTestError, SelfTestProbe, TsfClaim};
use crate::domain::EcPublicJwk;

// RFC 7515 §A.3.1, Figure 21 ("kty":"EC","crv":"P-256").
pub(crate) const KAT_PUBLIC_KEY_X: &str = "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU";
pub(crate) const KAT_PUBLIC_KEY_Y: &str = "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0";

// RFC 7515 §A.3.1, Figure 27 — JWS Compact Serialization (line breaks removed; the RFC
// wraps them for print layout only, they are not part of the value).
pub(crate) const KAT_VALID_JWS: &str = "eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ.DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1Q";

// Same header and payload, low bit of the signature's final byte flipped (Q -> A). Must
// be rejected.
pub(crate) const KAT_TAMPERED_JWS: &str = "eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ.DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1A";

// RFC 7515 §A.3.1, Figure 7 — the decoded JWS Payload, exactly (including the embedded
// \r\n from the RFC's own example).
pub(crate) const KAT_EXPECTED_PAYLOAD: &[u8] =
    b"{\"iss\":\"joe\",\r\n \"exp\":1300819380,\r\n \"http://example.com/is_root\":true}";

pub(crate) fn kat_public_jwk() -> Jwk {
    let ec_jwk = EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: KAT_PUBLIC_KEY_X.to_string(),
        y: KAT_PUBLIC_KEY_Y.to_string(),
        kid: String::new(),
    };
    Jwk::try_from(&ec_jwk).expect("pinned KAT public key must convert to a Jwk")
}

pub struct CryptoEs256KatProbe;

impl SelfTestProbe for CryptoEs256KatProbe {
    fn name(&self) -> &'static str {
        "crypto_es256_kat"
    }

    fn claim(&self) -> TsfClaim {
        TsfClaim::CryptographicLibraries
    }

    fn probe(&self) -> Result<(), SelfTestError> {
        let public_key = kat_public_jwk();

        let payload =
            hsmjose::jws_verify(KAT_VALID_JWS, &public_key).map_err(|_| SelfTestError {
                detail: "ES256 KAT: pinned positive vector failed to verify".to_string(),
            })?;
        if payload != KAT_EXPECTED_PAYLOAD {
            return Err(SelfTestError {
                detail: "ES256 KAT: positive vector verified but payload did not match".to_string(),
            });
        }

        match hsmjose::jws_verify(KAT_TAMPERED_JWS, &public_key) {
            Ok(_) => Err(SelfTestError {
                detail: "ES256 KAT: tampered signature was incorrectly accepted".to_string(),
            }),
            Err(_) => Ok(()),
        }
    }
}
