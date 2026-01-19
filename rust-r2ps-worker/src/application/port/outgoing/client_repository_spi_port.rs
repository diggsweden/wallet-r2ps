use crate::domain::HsmKey;
use crate::domain::{ClientMetadata, ServiceRequestError};

pub trait ClientRepositorySpiPort {
    fn client_metadata(&self, client_id: &str) -> Option<ClientMetadata>;
    fn store_metadata(&self, client_metadata: ClientMetadata) -> Result<(), ClientRepositoryError>;
    fn find_key(&self, client_id: &str, kid: &str) -> Result<HsmKey, ClientRepositoryError>;
    fn add_key(&self, client_id: &str, key: &HsmKey) -> Result<(), ClientRepositoryError>;
    fn delete_key(&self, client_id: &str, kid: &str) -> Result<(), ClientRepositoryError>;
}

#[derive(Debug)]
pub enum ClientRepositoryError {
    ClientNotFound,
    KeyNotFound,
}

impl From<ClientRepositoryError> for ServiceRequestError {
    fn from(error: ClientRepositoryError) -> Self {
        match error {
            ClientRepositoryError::ClientNotFound => ServiceRequestError::UnknownClient,
            ClientRepositoryError::KeyNotFound => ServiceRequestError::UnknownKey,
        }
    }
}
