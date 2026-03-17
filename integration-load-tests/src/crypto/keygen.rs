// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! EC P-256 key generation and JWK thumbprint computation.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use p256::ecdsa::SigningKey;
use sha2::{Digest, Sha256};

/// EC P-256 key pair with base64url-no-pad encoded coordinates.
#[derive(Clone, Debug)]
pub struct EcKeyPair {
    pub x: String,
    pub y: String,
    pub d: String,
    pub kid: String,
}

/// Generate a new EC P-256 key pair.
///
/// Returns an `EcKeyPair` with base64url-no-pad encoded x, y, d coordinates
/// and a kid computed as the JWK thumbprint (RFC 7638).
pub fn generate_ec_p256_keypair() -> EcKeyPair {
    let signing_key = SigningKey::random(&mut rand::rngs::OsRng);
    let public_key = signing_key.verifying_key();
    let ec_point = public_key.to_encoded_point(false);

    let x = URL_SAFE_NO_PAD.encode(ec_point.x().expect("X coordinate"));
    let y = URL_SAFE_NO_PAD.encode(ec_point.y().expect("Y coordinate"));
    let d = URL_SAFE_NO_PAD.encode(signing_key.to_bytes());
    let kid = compute_jwk_thumbprint(&x, &y);

    EcKeyPair { x, y, d, kid }
}

/// Compute the JWK thumbprint (RFC 7638) for an EC P-256 public key.
///
/// The thumbprint is `base64url(sha256(canonical_jwk))` where the canonical
/// JWK contains only `crv`, `kty`, `x`, `y` in lexicographic order.
pub fn compute_jwk_thumbprint(x: &str, y: &str) -> String {
    let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{}","y":"{}"}}"#, x, y);
    URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_keypair() {
        let kp = generate_ec_p256_keypair();
        assert_eq!(URL_SAFE_NO_PAD.decode(&kp.x).unwrap().len(), 32);
        assert_eq!(URL_SAFE_NO_PAD.decode(&kp.y).unwrap().len(), 32);
        assert_eq!(URL_SAFE_NO_PAD.decode(&kp.d).unwrap().len(), 32);
        assert_eq!(URL_SAFE_NO_PAD.decode(&kp.kid).unwrap().len(), 32);
    }

    #[test]
    fn test_thumbprint_deterministic() {
        let kp = generate_ec_p256_keypair();
        let kid2 = compute_jwk_thumbprint(&kp.x, &kp.y);
        assert_eq!(kp.kid, kid2);
    }
}
