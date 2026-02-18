use crate::domain::{DefaultCipherSuite, HsmKey, ServiceRequestError};
use generic_array::GenericArray;
use josekit::jwk::Jwk;
use josekit::jws::{JwsSigner, JwsVerifier};
use josekit::jwt::{self, JwtPayload};
use opaque_ke::ServerRegistrationLen;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

#[derive(Clone, Serialize, Deserialize)]
pub struct PasswordFile(pub GenericArray<u8, ServerRegistrationLen<DefaultCipherSuite>>);

// A distinct type with a suitable debug implementation
impl std::fmt::Debug for PasswordFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PasswordFile({} bytes)", self.0.len())
    }
}

impl PasswordFile {
    pub fn as_bytes(&self) -> &GenericArray<u8, ServerRegistrationLen<DefaultCipherSuite>> {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordFileEntry {
    pub password_file: PasswordFile,
    pub server_identifier: String,
    pub created_at: String, // ISO8601 timestamp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeyEntry {
    pub public_key: Jwk,
    pub password_files: Vec<PasswordFileEntry>,
    pub dev_authorization_code: Option<String>,
}

impl DeviceKeyEntry {
    pub fn kid(&self) -> Option<&str> {
        self.public_key.key_id()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceHsmState {
    pub version: u32,
    pub device_keys: Vec<DeviceKeyEntry>,
    pub hsm_keys: Vec<HsmKey>,
}

impl DeviceHsmState {
    pub fn serialize(&self) -> Result<Vec<u8>, ServiceRequestError> {
        match serde_json::to_vec(&self) {
            Ok(payload_vec) => Ok(payload_vec),
            Err(_) => Err(ServiceRequestError::SerializeStateError),
        }
    }

    // === Device key methods ===

    /// Find device key entry by kid
    pub fn find_device_key(&self, kid: &str) -> Option<&DeviceKeyEntry> {
        self.device_keys
            .iter()
            .find(|entry| entry.kid() == Some(kid))
    }

    /// Find mutable device key entry by kid
    pub fn find_device_key_mut(&mut self, kid: &str) -> Option<&mut DeviceKeyEntry> {
        self.device_keys
            .iter_mut()
            .find(|entry| entry.kid() == Some(kid))
    }

    /// Add device key entry, ensuring no duplicate kid
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

    /// Remove device key entry by kid
    pub fn remove_device_key(&mut self, kid: &str) -> Result<DeviceKeyEntry, ServiceRequestError> {
        let pos = self
            .device_keys
            .iter()
            .position(|entry| entry.kid() == Some(kid))
            .ok_or(ServiceRequestError::UnknownClient)?;

        Ok(self.device_keys.remove(pos))
    }

    // === HSM key methods ===

    /// Find HSM key by kid
    pub fn find_hsm_key(&self, kid: &str) -> Option<&HsmKey> {
        self.hsm_keys.iter().find(|key| key.kid() == kid)
    }

    /// Add HSM key, ensuring no duplicate kid
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

    /// Remove HSM key by kid
    pub fn remove_hsm_key(&mut self, kid: &str) -> Result<HsmKey, ServiceRequestError> {
        let pos = self
            .hsm_keys
            .iter()
            .position(|key| key.kid() == kid)
            .ok_or(ServiceRequestError::HsmKeyNotFound)?;

        Ok(self.hsm_keys.remove(pos))
    }

    // === Higher-level convenience methods ===

    /// Gets the latest password file for a given kid
    pub fn get_password_file(&self, kid: &str) -> Option<&PasswordFile> {
        self.find_device_key(kid)
            .and_then(|entry| entry.password_files.last())
            .map(|pf_entry| &pf_entry.password_file)
    }

    /// Adds a password file entry to the client key for the given kid.
    /// If `authorization_code` is provided, it must match `dev_authorization_code` on the entry,
    /// after which that code is cleared (consumed).
    pub fn add_password_file(
        &mut self,
        kid: &str,
        password_file: PasswordFile,
        server_identifier: String,
        authorization_code: Option<&str>,
    ) -> Result<(), ServiceRequestError> {
        use chrono::Utc;

        let timestamp = Utc::now().to_rfc3339();

        let password_file_entry = PasswordFileEntry {
            password_file,
            server_identifier,
            created_at: timestamp,
        };

        let entry = self
            .find_device_key_mut(kid)
            .ok_or(ServiceRequestError::UnknownClient)?;

        if let Some(code) = authorization_code {
            if entry.dev_authorization_code.as_deref() != Some(code) {
                return Err(ServiceRequestError::InvalidAuthorizationCode);
            }
            entry.dev_authorization_code = None;
        }

        entry.password_files.push(password_file_entry);
        Ok(())
    }

    // === JWS encoding/decoding ===

    /// Encode state as JWS using provided signer
    pub fn encode_to_jws(&self, signer: &dyn JwsSigner) -> Result<String, ServiceRequestError> {
        // Create JWT payload from state
        let payload_json = serde_json::to_string(&self).map_err(|e| {
            error!("Failed to serialize state: {:?}", e);
            ServiceRequestError::SerializeStateError
        })?;

        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&payload_json)
            .map_err(|e| {
            error!("Failed to create payload map: {:?}", e);
            ServiceRequestError::SerializeStateError
        })?;

        let payload = JwtPayload::from_map(map).map_err(|e| {
            error!("Failed to create JwtPayload: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        // Create JWS header
        let header = josekit::jws::JwsHeader::new();

        let token = jwt::encode_with_signer(&payload, &header, signer).map_err(|e| {
            error!("Failed to encode state JWS: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        Ok(token)
    }

    /// Decode state from JWS using provided verifier
    pub fn decode_from_jws(
        jws: &str,
        verifier: &dyn JwsVerifier,
    ) -> Result<Self, ServiceRequestError> {
        // Decode and verify JWT
        let (payload, _header) = jwt::decode_with_verifier(jws, verifier).map_err(|e| {
            error!("State JWS verification failed: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        let state: DeviceHsmState = serde_json::from_str(&payload.to_string()).map_err(|e| {
            error!("Failed to deserialize state: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        debug!("decoded state JWS: {:#?}", state);
        Ok(state)
    }
}
