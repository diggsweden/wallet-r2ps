use crate::domain::R2PsResponse;

pub trait R2psResponseSpiPort {
    fn send(&self, r2ps_response: R2PsResponse) -> Result<(), R2psResponseError>;
}

#[derive(Debug)]
pub enum R2psResponseError {
    ConnectionError,
    // TODO
}
