use crate::application::port::outgoing::jose_port;
use crate::application::session_key_spi_port::SessionKey;
use crate::domain::{
    EcPublicJwk, EncryptOption, InnerRequest, InnerResponse, OuterRequest, OuterResponse, TypedJwe,
    TypedJws, WorkerRequestError,
};
use tracing::{debug, error};

impl OuterRequest {
    pub fn from_jws(
        jws: &str,
        jose: &dyn jose_port::JosePort,
        key: &EcPublicJwk,
    ) -> Result<Self, WorkerRequestError> {
        let bytes = jose
            .jws_verify_device(jws, key)
            .map_err(|_| WorkerRequestError::OuterJwsError)?;
        serde_json::from_slice(&bytes).map_err(|e| {
            error!("Failed to deserialize outer request: {:?}", e);
            WorkerRequestError::OuterJwsError
        })
    }

    pub fn decrypt_inner(
        &self,
        jose: &dyn jose_port::JosePort,
        session_key: Option<&SessionKey>,
    ) -> Result<InnerRequest, WorkerRequestError> {
        let jwe = self
            .inner_jwe
            .as_ref()
            .ok_or(WorkerRequestError::InnerJweError)?;

        let peeked_kid = jose
            .peek_kid(jwe.as_str())
            .map_err(|_| WorkerRequestError::InnerJweError)?;
        debug!("Peeked inner JWE kid: {:?}", peeked_kid);

        let (bytes, enc_option) = match peeked_kid.as_deref() {
            Some("session") => {
                let key = session_key.ok_or(WorkerRequestError::UnknownSession)?;
                let bytes = jose
                    .jwe_decrypt(jwe.as_str(), jose_port::JweDecryptionKey::Session(key))
                    .map_err(|_| WorkerRequestError::DecryptionError)?;
                (bytes, EncryptOption::Session)
            }
            Some("device") => {
                let bytes = jose
                    .jwe_decrypt(jwe.as_str(), jose_port::JweDecryptionKey::Device)
                    .map_err(|_| WorkerRequestError::DecryptionError)?;
                (bytes, EncryptOption::Device)
            }
            _ => {
                error!("Unknown encryption option in JWE kid: {:?}", peeked_kid);
                return Err(WorkerRequestError::InnerJweError);
            }
        };

        let inner_request: InnerRequest =
            serde_json::from_slice(&bytes).map_err(|_| WorkerRequestError::InnerJweError)?;

        if inner_request.request_type.encrypt_option() != enc_option {
            error!(
                "Encryption option mismatch for {:?}: expected {:?}, got {:?}",
                inner_request.request_type,
                inner_request.request_type.encrypt_option(),
                enc_option
            );
            return Err(WorkerRequestError::InnerJweError);
        }

        Ok(inner_request)
    }
}

impl OuterResponse {
    pub fn sign(
        &self,
        jose: &dyn jose_port::JosePort,
    ) -> Result<TypedJws<OuterResponse>, WorkerRequestError> {
        let bytes = serde_json::to_vec(self).map_err(|e| {
            error!("Failed to serialize outer response: {:?}", e);
            WorkerRequestError::OuterJwsError
        })?;
        let jws_str = jose
            .jws_sign(&bytes)
            .map_err(|_| WorkerRequestError::OuterJwsError)?;
        Ok(TypedJws::new(jws_str))
    }
}

impl InnerResponse {
    pub fn encrypt(
        &self,
        jose: &dyn jose_port::JosePort,
        key: jose_port::JweEncryptionKey<'_>,
    ) -> Result<TypedJwe<InnerResponse>, WorkerRequestError> {
        let bytes = serde_json::to_vec(self).map_err(|e| {
            error!("Failed to serialize inner response: {:?}", e);
            WorkerRequestError::EncryptionError
        })?;
        let jwe_str = jose
            .jwe_encrypt(&bytes, key)
            .map_err(|_| WorkerRequestError::EncryptionError)?;
        Ok(TypedJwe::new(jwe_str))
    }
}
