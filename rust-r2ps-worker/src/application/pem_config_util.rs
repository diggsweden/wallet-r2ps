use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use pem::Pem;
use std::env;
use tracing::error;

pub fn load_pem_from_bas64_env(env_var_name: &str) -> Result<Pem, LoadPemError> {
    match env::var(env_var_name) {
        Ok(client_public_key_pem_base64) => {
            match BASE64_STANDARD.decode(&client_public_key_pem_base64) {
                Ok(decoded_bytes) => match pem::parse(&decoded_bytes) {
                    Ok(client_public_key) => Ok(client_public_key),
                    Err(e) => Err(LoadPemError::InvalidPem),
                },
                Err(e) => {
                    error!("Invalid client public key (base64 pem) : {}", e);
                    Err(LoadPemError::InvalidBase64Pem)
                }
            }
        }
        Err(e) => {
            error!("Invalid client public key (env variable) : {}", e);
            Err(LoadPemError::EnvError)
        }
    }
}

#[derive(Debug, Clone)]
pub enum LoadPemError {
    InvalidBase64Pem,
    InvalidPem,
    EnvError,
}
