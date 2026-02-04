use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use josekit::jwe;
use josekit::jwe::alg::direct::DirectJweAlgorithm;
use josekit::jwe::{ECDH_ES, JweHeader};
use pem::Pem;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use crate::application::session_key_spi_port::SessionKey;
use crate::domain::value_objects::InnerRequest;
use crate::domain::{EncryptOption, ServiceRequestError}; // Add this import

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InnerJwe(String);

impl InnerJwe {
    pub fn new(jwe: String) -> Self {
        Self(jwe)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    /// Peeks into the JWE header without decrypting and returns the kid (key ID) if present
    pub fn peek_kid(&self) -> Result<Option<String>, ServiceRequestError> {
        // Split JWE compact serialization (header.encrypted_key.iv.ciphertext.tag)
        let parts: Vec<&str> = self.0.split('.').collect();
        if parts.is_empty() {
            return Err(ServiceRequestError::JweError);
        }

        // Decode the header (first part)
        let header_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|_| ServiceRequestError::JweError)?;

        let header: serde_json::Value =
            serde_json::from_slice(&header_bytes).map_err(|_| ServiceRequestError::JweError)?;

        Ok(header
            .get("kid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    pub fn decrypt(
        &self,
        enc_option: EncryptOption,
        server_private_key: &Pem,
        session_key: Option<&SessionKey>,
    ) -> Result<InnerRequest, ServiceRequestError> {
        let payload = match enc_option {
            EncryptOption::Session => {
                let key = session_key.ok_or(ServiceRequestError::UnknownSession)?;
                self.decrypt_with_aes(key)?
            }
            EncryptOption::Device => self.decrypt_with_ec_pem(server_private_key)?,
        };

        serde_json::from_slice(&payload).map_err(|e| {
            error!("Failed to deserialize InnerRequest: {:?}", e);
            ServiceRequestError::JweError
        })
    }

    fn decrypt_with_ec_pem(&self, private_key: &Pem) -> Result<Vec<u8>, ServiceRequestError> {
        let decrypter = ECDH_ES.decrypter_from_pem(pem::encode(private_key))?;
        let (payload, header) = jwe::deserialize_compact(&self.0, &decrypter)?;

        debug!("Inner JWE header: {:#?}", header);

        Ok(payload)
    }

    fn decrypt_with_aes(&self, session_key: &SessionKey) -> Result<Vec<u8>, ServiceRequestError> {
        let decrypter = DirectJweAlgorithm::Dir
            .decrypter_from_bytes(session_key.as_ref())
            .map_err(|e| {
                error!("Failed to create decrypter: {:?}", e);
                ServiceRequestError::JweError
            })?;

        let (payload, header) = jwe::deserialize_compact(&self.0, &decrypter).map_err(|e| {
            error!("Failed to decrypt: {:?}", e);
            ServiceRequestError::JweError
        })?;

        debug!("Inner JWE header: {:#?}", header);

        Ok(payload)
    }

    pub fn encrypt(payload: &[u8], session_key: &SessionKey) -> Result<Self, ServiceRequestError> {
        let mut header = JweHeader::new();
        header.set_algorithm("dir");
        header.set_content_encryption("A256GCM");
        header.set_key_id("session");

        let encrypter = DirectJweAlgorithm::Dir
            .encrypter_from_bytes(session_key.as_ref())
            .map_err(|e| {
                error!("Failed to create encrypter: {:?}", e);
                ServiceRequestError::Unknown
            })?;

        let jwe = jwe::serialize_compact(payload, &header, &encrypter).map_err(|e| {
            error!("Failed to encrypt: {:?}", e);
            ServiceRequestError::Unknown
        })?;

        Ok(Self(jwe))
    }
}
