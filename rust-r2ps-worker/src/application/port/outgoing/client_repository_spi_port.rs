use crate::domain::ClientMetadata;
use crate::infrastructure::hsm_wrapper::HsmKey;

pub trait ClientRepositorySpiPort {
    fn client_metadata(&self, client_id: &str) -> Option<ClientMetadata>;
    fn store_metadata(&self, client_metadata: ClientMetadata) -> Result<(), ClientRepositoryError>;
    fn add_key(&self, client_id: &str, key: &HsmKey) -> Result<(), ClientRepositoryError>;
}

#[derive(Debug)]
pub enum ClientRepositoryError {
    ConnectionError,
    // TODO
}
