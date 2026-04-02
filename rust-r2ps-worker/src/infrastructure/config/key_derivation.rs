use hkdf::Hkdf;
use p256::SecretKey;
use p256::elliptic_curve::hash2curve::FromOkm;
use sha2::Sha512;

#[derive(Debug)]
pub enum DeriveError {
    HkdfError,
    InvalidScalar,
}

impl std::fmt::Display for DeriveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HkdfError => write!(f, "HKDF expansion failed"),
            Self::InvalidScalar => write!(f, "derived scalar is not a valid P-256 key"),
        }
    }
}

impl std::error::Error for DeriveError {}

/// Derive a P-256 secret key from HMAC-SHA512 PRF output and a domain separator.
/// HKDF-Expand(hash=SHA-512, ikm=hmac_output, info=domain_sep, L=48) → hash_to_field → scalar.
///
/// HKDF-Extract is omitted: the IKM is a 512-bit output from a non-extractable HSM HMAC key,
/// already a uniform PRF value. There is no additional entropy to condition, so Extract adds
/// nothing here. The sole purpose of HKDF is to produce 48 bytes of uniform material suitable
/// for hash_to_field from the HMAC output.
pub fn derive_scalar(hmac_output: &[u8], domain_sep: &str) -> Result<SecretKey, DeriveError> {
    let hk = Hkdf::<Sha512>::new(None, hmac_output);
    let mut okm = [0u8; 48];
    hk.expand(domain_sep.as_bytes(), &mut okm)
        .map_err(|_| DeriveError::HkdfError)?;

    let scalar =
        p256::Scalar::from_okm(p256::elliptic_curve::generic_array::GenericArray::from_slice(&okm));
    SecretKey::from_bytes(&scalar.to_bytes()).map_err(|_| DeriveError::InvalidScalar)
}
