use crate::domain::ClientMetadata;

pub trait ClientRepositorySpiPort {
    fn client_metadata(&self, client_id: &str) -> Option<ClientMetadata>;
    fn store_metadata(&self, client_metadata: ClientMetadata) -> Result<(), ClientRepositoryError>;
}

#[derive(Debug)]
pub enum ClientRepositoryError {
    ConnectionError,
    // TODO
}