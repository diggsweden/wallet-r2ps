use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// An encrypted (wrapped) private key stored in the HSM state.
/// Serialized as a base64-encoded string of the wrapped private key bytes.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
pub struct WrappedPrivateKey(Vec<u8>);

impl WrappedPrivateKey {
    pub fn new(key: Vec<u8>) -> Self {
        Self(key)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl std::fmt::Debug for WrappedPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WrappedPrivateKey({} bytes)", self.0.len())
    }
}

/// A key pair managed by the HSM, consisting of a wrapped (encrypted) private key
/// and its corresponding EC public key in JWK format.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct HsmKey {
    /// The wrapped (encrypted) private key bytes
    pub wrapped_private_key: WrappedPrivateKey,
    /// The public key in EC JWK format
    pub public_key_jwk: EcPublicJwk,
    /// Timestamp when this key was created
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "date-time"))]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl HsmKey {
    pub fn kid(&self) -> &str {
        &self.public_key_jwk.kid
    }
}

/// An elliptic curve public key in JWK (JSON Web Key) format.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EcPublicJwk {
    /// Key type, always "EC" for elliptic curve keys
    #[cfg_attr(feature = "openapi", schema(example = "EC"))]
    pub kty: String,
    /// The curve name (e.g. "P-256", "P-384", "P-521")
    #[cfg_attr(feature = "openapi", schema(example = "P-256"))]
    pub crv: String,
    /// The x coordinate (base64url-encoded)
    pub x: String,
    /// The y coordinate (base64url-encoded)
    pub y: String,
    /// Key identifier
    pub kid: String,
}
