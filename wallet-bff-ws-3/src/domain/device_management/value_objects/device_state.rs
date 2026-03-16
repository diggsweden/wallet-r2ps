use std::fmt;

use base64::Engine;
use serde::{Deserialize, Serialize};

/// JWS-signed device state blob.
///
/// This value object wraps a JWS string representing the cryptographic state
/// of a device. The state is produced by the HSM worker. This service can parse
/// the JWS payload to extract device public keys for WebSocket authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceState(String);

impl DeviceState {
    /// Create a new `DeviceState` from a JWS string.
    pub fn new(jws: impl Into<String>) -> Result<Self, DeviceStateError> {
        let jws = jws.into();
        if jws.is_empty() {
            return Err(DeviceStateError::EmptyJws);
        }
        Ok(Self(jws))
    }

    /// Return the JWS string.
    pub fn as_jws(&self) -> &str {
        &self.0
    }

    /// Consume self and return the inner JWS string.
    pub fn into_jws(self) -> String {
        self.0
    }

    /// Decode the JWS payload (without signature verification) and parse
    /// the device state structure. Use [`verify_and_parse_payload`] when
    /// you have the HSM public key and want cryptographic verification.
    pub fn parse_payload(&self) -> Result<DeviceStatePayload, DeviceStateError> {
        let payload_b64 = self.extract_payload_b64()?;
        let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|e| DeviceStateError::PayloadDecodeError(e.to_string()))?;
        let payload: DeviceStatePayload = serde_json::from_slice(&payload_bytes)
            .map_err(|e| DeviceStateError::PayloadParseError(e.to_string()))?;
        Ok(payload)
    }

    /// Verify the JWS signature using the HSM public key (EC P-256) and parse
    /// the payload. The `hsm_public_key_jwk` should be a JWK JSON string.
    pub fn verify_and_parse_payload(
        &self,
        hsm_public_key_jwk: &str,
    ) -> Result<DeviceStatePayload, DeviceStateError> {
        use josekit::{jwk::Jwk, jws::ES256};

        let jwk: Jwk = Jwk::from_bytes(hsm_public_key_jwk.as_bytes()).map_err(|e| {
            DeviceStateError::JwsVerificationError(format!("invalid HSM JWK: {}", e))
        })?;

        let verifier = ES256.verifier_from_jwk(&jwk).map_err(|e| {
            DeviceStateError::JwsVerificationError(format!("failed to create verifier: {}", e))
        })?;

        let (payload_bytes, _header) = josekit::jws::deserialize_compact(self.as_jws(), &verifier)
            .map_err(|e| {
                DeviceStateError::JwsVerificationError(format!(
                    "JWS signature verification failed: {}",
                    e
                ))
            })?;

        let payload: DeviceStatePayload = serde_json::from_slice(&payload_bytes)
            .map_err(|e| DeviceStateError::PayloadParseError(e.to_string()))?;
        Ok(payload)
    }

    /// Extract the base64url-encoded payload segment from the compact JWS.
    fn extract_payload_b64(&self) -> Result<&str, DeviceStateError> {
        let parts: Vec<&str> = self.0.split('.').collect();
        if parts.len() != 3 {
            return Err(DeviceStateError::InvalidJwsFormat);
        }
        Ok(parts[1])
    }
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Truncate for security — don't log full JWS
        if self.0.len() > 20 {
            write!(f, "DeviceState({}...)", &self.0[..20])
        } else {
            write!(f, "DeviceState({})", &self.0)
        }
    }
}

/// Parsed payload of a DeviceState JWS.
///
/// Structure:
/// ```json
/// {
///   "version": 1,
///   "device_keys": [{ "public_key": { ... }, "password_files": [], "dev_authorization_code": "..." }],
///   "hsm_keys": []
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStatePayload {
    pub version: u64,
    pub device_keys: Vec<DeviceKeyEntry>,
    #[serde(default)]
    pub hsm_keys: Vec<serde_json::Value>,
}

impl DeviceStatePayload {
    /// Extract the first device public key from the payload.
    pub fn first_public_key(&self) -> Option<&EcPublicKeyData> {
        self.device_keys.first().map(|k| &k.public_key)
    }

    /// Find a device public key by `kid`.
    pub fn find_public_key_by_kid(&self, kid: &str) -> Option<&EcPublicKeyData> {
        self.device_keys
            .iter()
            .find(|k| k.public_key.kid == kid)
            .map(|k| &k.public_key)
    }
}

/// A single device key entry in the DeviceState payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeyEntry {
    pub public_key: EcPublicKeyData,
    #[serde(default)]
    pub password_files: Vec<serde_json::Value>,
    /// Present in the initial device state; may be `null` in states
    /// returned by the HSM worker after processing requests.
    #[serde(default)]
    pub dev_authorization_code: Option<String>,
}

/// EC P-256 public key data as found in the DeviceState JWS payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcPublicKeyData {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    pub kid: String,
}

/// Errors that can occur when working with a `DeviceState`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DeviceStateError {
    #[error("device state JWS must not be empty")]
    EmptyJws,

    #[error("invalid JWS format: expected 3 dot-separated parts")]
    InvalidJwsFormat,

    #[error("failed to decode JWS payload: {0}")]
    PayloadDecodeError(String),

    #[error("failed to parse JWS payload: {0}")]
    PayloadParseError(String),

    #[error("JWS verification error: {0}")]
    JwsVerificationError(String),
}
