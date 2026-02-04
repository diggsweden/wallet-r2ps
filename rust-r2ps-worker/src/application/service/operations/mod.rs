pub mod authentication;
pub mod hsm;
pub mod session;

use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::pending_auth_spi_port::PendingAuthSpiPort;
use crate::application::session_key_spi_port::SessionKeySpiPort;
use crate::domain::{DefaultCipherSuite, OperationId, ServiceRequestError, SessionId};
use opaque_ke::ServerSetup;
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use authentication::{
    AuthenticateFinishOperation, AuthenticateStartOperation, RegisterFinishOperation,
    RegisterStartOperation,
};
use hsm::{HsmDeleteKeyOperation, HsmGenerateKeyOperation, HsmListKeysOperation, HsmSignOperation};
use session::SessionEndOperation;

#[derive(Debug)]
pub struct OperationContext {
    pub request_id: String,
    pub wallet_id: String,
    pub device_id: String,
    pub state: crate::domain::DeviceHsmState,
    pub outer_request: crate::domain::value_objects::r2ps::OuterRequest,
    pub inner_request: crate::domain::value_objects::r2ps::InnerRequest,
    pub session_id: Option<SessionId>,
}

pub struct OperationResult {
    pub state: crate::domain::DeviceHsmState,
    pub data: crate::domain::InnerResponseData,
    pub session_id: Option<SessionId>,
}

impl OperationResult {
    /// Creates an InnerResponse from this OperationResult with the serialized response data
    pub fn to_inner_response(
        &self,
        serialized_data: String,
        ttl: Option<std::time::Duration>,
    ) -> crate::domain::value_objects::r2ps::InnerResponse {
        use crate::domain::value_objects::r2ps::{InnerResponse, Status, to_iso8601_duration};

        InnerResponse {
            version: 1,
            data: Some(serialized_data),
            expires_in: ttl.map(to_iso8601_duration),
            status: Status::Ok,
        }
    }
}

/// Trait for service operations that can be executed
pub trait ServiceOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError>;
}

/// Contains all operation handlers
pub struct OperationDispatcher {
    authenticate_start_op: AuthenticateStartOperation,
    authenticate_finish_op: AuthenticateFinishOperation,
    register_start_op: RegisterStartOperation,
    register_finish_op: RegisterFinishOperation,
    hsm_ecdsa_op: HsmSignOperation,
    hsm_keygen_op: HsmGenerateKeyOperation,
    hsm_delete_key_op: HsmDeleteKeyOperation,
    hsm_list_keys_op: HsmListKeysOperation,
    session_end_op: SessionEndOperation,
}

impl OperationDispatcher {
    /// Creates a new OperationDispatcher with all operation handlers initialized
    pub fn from_dependencies(
        server_setup: ServerSetup<DefaultCipherSuite>,
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
        hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
        pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
    ) -> Self {
        Self {
            authenticate_start_op: AuthenticateStartOperation::new(
                server_setup.clone(),
                pending_auth_spi_port.clone(),
            ),
            authenticate_finish_op: AuthenticateFinishOperation::new(
                pending_auth_spi_port.clone(),
                session_key_spi_port.clone(),
            ),
            register_start_op: RegisterStartOperation::new(server_setup.clone()),
            register_finish_op: RegisterFinishOperation::new(),
            hsm_ecdsa_op: HsmSignOperation::new(hsm_spi_port.clone()),
            hsm_keygen_op: HsmGenerateKeyOperation::new(hsm_spi_port.clone()),
            hsm_delete_key_op: HsmDeleteKeyOperation,
            hsm_list_keys_op: HsmListKeysOperation,
            session_end_op: SessionEndOperation::new(session_key_spi_port.clone()),
        }
    }

    /// Dispatches the request to the appropriate operation handler
    pub fn dispatch(
        &self,
        context: OperationContext,
    ) -> Result<OperationResult, ServiceRequestError> {
        let start = Instant::now();

        let request_type = &context.inner_request.request_type.clone();
        debug!("Requested Operation: {:?}", request_type);

        let result = match request_type {
            OperationId::AuthenticateStart => self.authenticate_start_op.execute(context),
            OperationId::AuthenticateFinish => self.authenticate_finish_op.execute(context),
            OperationId::RegisterStart => self.register_start_op.execute(context),
            OperationId::RegisterFinish => self.register_finish_op.execute(context),
            OperationId::HsmSign => self.hsm_ecdsa_op.execute(context),
            OperationId::HsmGenerateKey => self.hsm_keygen_op.execute(context),
            OperationId::HsmDeleteKey => self.hsm_delete_key_op.execute(context),
            OperationId::HsmListKeys => self.hsm_list_keys_op.execute(context),
            OperationId::EndSession => self.session_end_op.execute(context),
            OperationId::PinChange => Err(ServiceRequestError::Unknown),
            OperationId::HsmEcdh => Err(ServiceRequestError::Unknown),
            OperationId::Store => Err(ServiceRequestError::Unknown),
            OperationId::Retrieve => Err(ServiceRequestError::Unknown),
            OperationId::Log => Err(ServiceRequestError::Unknown),
            OperationId::GetLog => Err(ServiceRequestError::Unknown),
            OperationId::Info => Err(ServiceRequestError::Unknown),
        };

        let elapsed = start.elapsed();
        debug!(
            "Request {:?} inner execute time: {} ms",
            request_type,
            elapsed.as_millis()
        );

        result
    }
}
