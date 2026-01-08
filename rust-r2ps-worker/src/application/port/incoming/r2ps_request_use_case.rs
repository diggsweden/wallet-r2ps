use crate::domain::{R2psRequest, R2psRequestError};

pub trait R2psRequestUseCase {
    fn execute(&self, r2ps_request: R2psRequest) -> Result<R2psRequestId, R2psRequestError>;
}

pub type R2psRequestId = String;
