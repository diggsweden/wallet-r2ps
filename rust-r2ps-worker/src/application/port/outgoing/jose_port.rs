use crate::application::port::outgoing::session_state_spi_port::SessionKey;
use crate::domain::EcPublicJwk;

#[derive(Debug)]
pub enum JoseError {
    SignError,
    VerifyError,
    EncryptError,
    DecryptError,
    InvalidKey,
}

pub enum JweEncryptionKey<'a> {
    Session(&'a SessionKey),
    Device(&'a EcPublicJwk),
}

/// Device variant uses the server private key held by the adapter.
pub enum JweDecryptionKey<'a> {
    Device,
    Session(&'a SessionKey),
}

#[cfg_attr(test, mockall::automock)]
pub trait JosePort: Send + Sync {
    fn jws_sign(&self, payload_json: &[u8]) -> Result<String, JoseError>;
    fn jws_verify_server(&self, jws: &str) -> Result<Vec<u8>, JoseError>;
    fn jws_verify_device(&self, jws: &str, key: &EcPublicJwk) -> Result<Vec<u8>, JoseError>;
    fn jwe_encrypt<'a>(
        &self,
        payload: &[u8],
        key: JweEncryptionKey<'a>,
    ) -> Result<String, JoseError>;
    fn jwe_decrypt<'a>(&self, jwe: &str, key: JweDecryptionKey<'a>) -> Result<Vec<u8>, JoseError>;
    fn peek_kid(&self, compact: &str) -> Result<Option<String>, JoseError>;
    fn jws_public_key(&self) -> &EcPublicJwk;
    fn jws_kid(&self) -> &str;
}
