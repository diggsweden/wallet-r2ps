use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use pem::Pem;
use tracing::{debug, error};

pub fn load_pem_from_base64(pem_b64: &str) -> Result<Pem, LoadPemError> {
    match BASE64_STANDARD.decode(pem_b64) {
        Ok(decoded_bytes) => match pem::parse(&decoded_bytes) {
            Ok(client_public_key) => Ok(client_public_key),
            Err(e) => {
                error!("Invalid PEM: {:?}", e);
                debug!(
                    "Decoded PEM content: {:?}",
                    String::from_utf8_lossy(&decoded_bytes)
                );
                Err(LoadPemError::InvalidPem)
            }
        },
        Err(e) => {
            error!("Invalid base64: {}", e);
            Err(LoadPemError::InvalidBase64Pem)
        }
    }
}

#[derive(Debug, Clone)]
pub enum LoadPemError {
    InvalidBase64Pem,
    InvalidPem,
    EnvError,
}
