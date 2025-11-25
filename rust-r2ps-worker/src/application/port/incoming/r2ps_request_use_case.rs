use crate::domain::{R2psRequest};

pub trait R2psRequestUseCase {
        fn execute(&self, r2ps_request: R2psRequest) -> Result<R2psRequestId, R2psRequestError>;
}

#[derive(Debug)]
pub enum R2psRequestError {
    ConnectionError,
    // TODO
}

pub type R2psRequestId = String;