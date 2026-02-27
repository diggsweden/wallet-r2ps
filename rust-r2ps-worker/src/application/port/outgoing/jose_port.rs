use crate::application::session_key_spi_port::SessionKey;
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

pub trait JosePort: Send + Sync {
    fn jws_sign(&self, payload_json: &[u8]) -> Result<String, JoseError>;
    fn jws_verify_server(&self, jws: &str) -> Result<Vec<u8>, JoseError>;
    fn jws_verify_device(&self, jws: &str, key: &EcPublicJwk) -> Result<Vec<u8>, JoseError>;
    fn jwe_encrypt(&self, payload: &[u8], key: JweEncryptionKey<'_>) -> Result<String, JoseError>;
    fn jwe_decrypt(&self, jwe: &str, key: JweDecryptionKey<'_>) -> Result<Vec<u8>, JoseError>;
    fn peek_kid(&self, compact: &str) -> Result<Option<String>, JoseError>;
}
