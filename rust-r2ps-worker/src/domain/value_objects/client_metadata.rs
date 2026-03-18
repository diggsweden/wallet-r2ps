use crate::domain::{EcPublicJwk, HsmKey, ServiceRequestError};
use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Custom serde module for Vec<u8> using base64url-no-pad encoding for deterministic hashing.
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

/// An OPAQUE server registration file, containing the server's share of the password credential.
/// Serialized as base64url-no-pad for deterministic hashing.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String, format = "byte"))]
pub struct PasswordFile(#[serde(with = "base64url_bytes")] pub Vec<u8>);

impl std::fmt::Debug for PasswordFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PasswordFile({} bytes)", self.0.len())
    }
}

impl PasswordFile {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// A timestamped OPAQUE password file entry, binding a password credential
/// to a specific server identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PasswordFileEntry {
    pub password_file: PasswordFile,
    pub server_identifier: String,
    pub created_at: String,
}

/// A registered device key with its associated OPAQUE password files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeviceKeyEntry {
    pub public_key: EcPublicJwk,
    pub password_files: Vec<PasswordFileEntry>,
    pub dev_authorization_code: Option<String>,
}

impl DeviceKeyEntry {
    pub fn kid(&self) -> Option<&str> {
        Some(self.public_key.kid.as_str())
    }
}

/// The complete persisted state for a device.
/// Now server-owned in PostgreSQL with monotonic versioning and hash chain integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DeviceHsmState {
    /// Monotonic state version, incremented per mutation (0 = genesis)
    pub version: u64,
    pub device_keys: Vec<DeviceKeyEntry>,
    pub hsm_keys: Vec<HsmKey>,
}

impl DeviceHsmState {
    pub fn serialize(&self) -> Result<Vec<u8>, ServiceRequestError> {
        serde_json::to_vec(&self).map_err(|_| ServiceRequestError::SerializeStateError)
    }

    // === Device key methods ===

    pub fn find_device_key(&self, kid: &str) -> Option<&DeviceKeyEntry> {
        self.device_keys
            .iter()
            .find(|entry| entry.kid() == Some(kid))
    }

    pub fn find_device_key_mut(&mut self, kid: &str) -> Option<&mut DeviceKeyEntry> {
        self.device_keys
            .iter_mut()
            .find(|entry| entry.kid() == Some(kid))
    }

    pub fn add_device_key(&mut self, entry: DeviceKeyEntry) -> Result<(), ServiceRequestError> {
        let kid = entry.kid().ok_or(ServiceRequestError::InvalidPublicKey)?;

        if kid.is_empty() {
            return Err(ServiceRequestError::InvalidPublicKey);
        }

        if self.find_device_key(kid).is_some() {
            return Err(ServiceRequestError::DuplicateKey);
        }

        self.device_keys.push(entry);
        Ok(())
    }

    pub fn remove_device_key(&mut self, kid: &str) -> Result<DeviceKeyEntry, ServiceRequestError> {
        let pos = self
            .device_keys
            .iter()
            .position(|entry| entry.kid() == Some(kid))
            .ok_or(ServiceRequestError::UnknownClient)?;

        Ok(self.device_keys.remove(pos))
    }

    // === HSM key methods ===

    pub fn find_hsm_key(&self, kid: &str) -> Option<&HsmKey> {
        self.hsm_keys.iter().find(|key| key.kid() == kid)
    }

    pub fn add_hsm_key(&mut self, key: HsmKey) -> Result<(), ServiceRequestError> {
        let kid = key.kid();

        if kid.is_empty() {
            return Err(ServiceRequestError::InvalidPublicKey);
        }

        if self.find_hsm_key(kid).is_some() {
            return Err(ServiceRequestError::DuplicateKey);
        }

        self.hsm_keys.push(key);
        Ok(())
    }

    pub fn remove_hsm_key(&mut self, kid: &str) -> Result<HsmKey, ServiceRequestError> {
        let pos = self
            .hsm_keys
            .iter()
            .position(|key| key.kid() == kid)
            .ok_or(ServiceRequestError::HsmKeyNotFound)?;

        Ok(self.hsm_keys.remove(pos))
    }

    // === Higher-level convenience methods ===

    pub fn get_password_file(&self, kid: &str) -> Option<&PasswordFile> {
        self.find_device_key(kid)
            .and_then(|entry| entry.password_files.last())
            .map(|pf_entry| &pf_entry.password_file)
    }

    pub fn set_password_file(
        &mut self,
        kid: &str,
        password_file_entry: PasswordFileEntry,
        authorization_code: Option<&str>,
    ) -> Result<(), ServiceRequestError> {
        let entry = self
            .find_device_key_mut(kid)
            .ok_or(ServiceRequestError::UnknownClient)?;

        if let Some(code) = authorization_code {
            if entry.dev_authorization_code.as_deref() != Some(code) {
                return Err(ServiceRequestError::InvalidAuthorizationCode);
            }
            entry.dev_authorization_code = None;
        }

        entry.password_files = vec![password_file_entry];
        Ok(())
    }
}
