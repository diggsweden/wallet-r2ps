use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Custom serde for Vec<u8> using base64url-no-pad for deterministic hashing.
mod base64url_bytes {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let encoded = BASE64_URL_SAFE_NO_PAD.encode(bytes);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(deserializer)?;
        BASE64_URL_SAFE_NO_PAD
            .decode(&s)
            .map_err(serde::de::Error::custom)
    }
}

/// An encrypted (wrapped) private key stored in the HSM state.
/// Serialized as base64url-no-pad for deterministic hashing.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
pub struct WrappedPrivateKey(#[serde(with = "base64url_bytes")] Vec<u8>);

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

/// A key pair managed by the HSM.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct HsmKey {
    pub wrapped_private_key: WrappedPrivateKey,
    pub public_key_jwk: EcPublicJwk,
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "date-time"))]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl HsmKey {
    pub fn kid(&self) -> &str {
        &self.public_key_jwk.kid
    }
}

/// An elliptic curve public key in JWK format.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EcPublicJwk {
    #[cfg_attr(feature = "openapi", schema(example = "EC"))]
    pub kty: String,
    #[cfg_attr(feature = "openapi", schema(example = "P-256"))]
    pub crv: String,
    pub x: String,
    pub y: String,
    pub kid: String,
}
