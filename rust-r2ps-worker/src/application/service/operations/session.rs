use super::{OperationContext, OperationResult, ServiceOperation};
use crate::application::session_key_spi_port::SessionKeySpiPort;
use crate::domain::value_objects::r2ps::PakeResponse;
use crate::domain::{InnerResponseData, ServiceRequestError};
use std::sync::Arc;

pub struct SessionEndOperation {
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
}

impl SessionEndOperation {
    pub fn new(session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>) -> Self {
        Self {
            session_key_spi_port,
        }
    }
}

impl ServiceOperation for SessionEndOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let session_id = context
            .session_id
            .as_ref()
            .ok_or(ServiceRequestError::UnknownSession)?;

        self.session_key_spi_port
            .end_session(&session_id)
            .map_err(|_| ServiceRequestError::UnknownSession)?;

        let payload = PakeResponse {
            task: None,
            data: None,
        };

        Ok(OperationResult {
            state: context.state,
            data: InnerResponseData::new(payload)?,
            session_id: context.session_id,
        })
    }
}
