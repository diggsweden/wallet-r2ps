use crate::application::port::outgoing::session_state_spi_port::SessionKey;
use crate::application::service::operations::OperationContext;
use crate::domain::{EcPublicJwk, OperationId};

#[derive(Debug, Clone)]
pub struct ResponseContext {
    pub correlation_id: String,
    pub device_id: String,
    pub request_id: Option<String>,
    pub request_type: OperationId,
    pub session_key: Option<SessionKey>,
    pub device_public_key: EcPublicJwk,
}

#[derive(Debug)]
pub struct WorkerInput {
    pub operation_context: OperationContext,
    pub response_context: ResponseContext,
}
