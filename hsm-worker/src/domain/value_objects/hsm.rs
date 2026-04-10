// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

pub use hsm_common::EcPublicJwk;

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
    /// Label of the AES wrap key used to wrap this private key
    #[serde(default)]
    pub wrap_key_label: String,
    /// Timestamp when this key was created
    #[cfg_attr(feature = "openapi", schema(value_type = String, format = "date-time"))]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl HsmKey {
    pub fn kid(&self) -> &str {
        &self.public_key_jwk.kid
    }
}
