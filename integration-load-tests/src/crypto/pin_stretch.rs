// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! PIN stretching implementation matching the Android `access-mechanism` library
//! and the `opaque-ke-wasm` crate.
//!
//! The stretching process:
//! 1. Hash the PIN to a P-256 curve point using RFC 9380 hash-to-curve
//!    with DST = "SE_EIDAS_WALLET_PIN_HARDENING"
//! 2. ECDH key agreement between the PIN stretch private key and the curve point
//! 3. HKDF-SHA256(ikm=shared_secret, salt=None, info=empty) -> 32 bytes

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint};
use p256::elliptic_curve::PrimeField;
use p256::{AffinePoint, EncodedPoint, NonZeroScalar, ProjectivePoint, PublicKey, Scalar};
use sha2::Sha256;
use voprf::Group;

/// Domain separation tag for PIN hash-to-curve, matching the Android library.
const PIN_STRETCH_DST: &[u8] = b"SE_EIDAS_WALLET_PIN_HARDENING";

/// Stretch a PIN into a 32-byte key suitable for OPAQUE.
///
/// This matches the Android `OpaqueCryptoManager.stretchPin()` implementation:
/// 1. hash_to_curve_p256_sha256(pin, DST) -> compressed point
/// 2. ECDH(pin_stretch_private_key, point) -> shared secret (x-coordinate)
/// 3. HKDF-SHA256(ikm=shared_secret, salt=None, info=empty) -> 32 bytes
pub fn stretch_pin(pin: &str, pin_stretch_d: &str) -> Result<Vec<u8>> {
    // Step 1: Hash PIN to P-256 curve point
    let compressed_point = hash_to_curve_p256(pin.as_bytes())?;

    // Step 2: Decode compressed point to an affine point
    let encoded_point = EncodedPoint::from_bytes(&compressed_point).context("Bad encoded point")?;
    let affine =
        Option::from(AffinePoint::from_encoded_point(&encoded_point)).context("Bad curve point")?;
    let pin_public_key = PublicKey::from_affine(affine).context("Bad public key")?;

    // Step 3: Decode the PIN stretch private key
    let d_bytes = URL_SAFE_NO_PAD
        .decode(pin_stretch_d)
        .context("Bad base64url for d")?;
    let scalar = Option::from(Scalar::from_repr(*p256::FieldBytes::from_slice(&d_bytes)))
        .context("Invalid scalar")?;
    let non_zero: NonZeroScalar =
        Option::from(NonZeroScalar::new(scalar)).context("Private key scalar is zero")?;

    // Step 4: ECDH — compute shared secret (x-coordinate of sk * point)
    let pin_projective = ProjectivePoint::from(*pin_public_key.as_affine());
    let shared_point = (pin_projective * *non_zero).to_affine();
    let shared_point_encoded = shared_point.to_encoded_point(false);
    let shared_secret = shared_point_encoded
        .x()
        .context("ECDH produced identity point")?;

    // Step 5: HKDF-SHA256(ikm=shared_secret, salt=None, info=empty) -> 32 bytes
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);
    let mut output = [0u8; 32];
    hkdf.expand(&[], &mut output)
        .map_err(|e| anyhow::anyhow!("HKDF expand failed: {:?}", e))?;

    Ok(output.to_vec())
}

/// Hash input bytes to a P-256 curve point using RFC 9380 hash-to-curve.
fn hash_to_curve_p256(input: &[u8]) -> Result<Vec<u8>> {
    let point = p256::NistP256::hash_to_curve::<sha2::Sha256>(&[input], &[PIN_STRETCH_DST])
        .map_err(|e| anyhow::anyhow!("hash_to_curve failed: {:?}", e))?;
    Ok(point.to_encoded_point(true).as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keygen;

    #[test]
    fn test_stretch_pin_produces_32_bytes() {
        let kp = keygen::generate_ec_p256_keypair();
        let result = stretch_pin("123456", &kp.d).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_stretch_pin_deterministic() {
        let kp = keygen::generate_ec_p256_keypair();
        let r1 = stretch_pin("123456", &kp.d).unwrap();
        let r2 = stretch_pin("123456", &kp.d).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_stretch_pin_different_pins() {
        let kp = keygen::generate_ec_p256_keypair();
        let r1 = stretch_pin("123456", &kp.d).unwrap();
        let r2 = stretch_pin("654321", &kp.d).unwrap();
        assert_ne!(r1, r2);
    }

    #[test]
    fn test_stretch_pin_different_keys() {
        let kp1 = keygen::generate_ec_p256_keypair();
        let kp2 = keygen::generate_ec_p256_keypair();
        let r1 = stretch_pin("123456", &kp1.d).unwrap();
        let r2 = stretch_pin("123456", &kp2.d).unwrap();
        assert_ne!(r1, r2);
    }
}
