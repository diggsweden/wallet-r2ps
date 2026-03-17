pub mod authentication;
pub mod hsm;
pub mod session;
pub mod state_init;

use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::pake_port::PakePort;
use crate::application::port::outgoing::session_state_spi_port::SessionState;
use crate::domain::{OperationId, ServiceRequestError, SessionId};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

pub use crate::application::port::outgoing::session_state_spi_port::SessionTransition;

use authentication::{
    AuthenticateFinishOperation, AuthenticateStartOperation, PinChangeFinishOperation,
    PinChangeStartOperation, RegisterFinishOperation, RegisterStartOperation,
};
use hsm::{HsmDeleteKeyOperation, HsmGenerateKeyOperation, HsmListKeysOperation, HsmSignOperation};
use session::SessionEndOperation;
use state_init::StateInitOperation;

#[derive(Debug)]
pub struct OperationContext {
    pub correlation_id: String,
    pub device_id: String,
    pub state: crate::domain::DeviceHsmState,
    pub outer_request: crate::domain::value_objects::r2ps::OuterRequest,
    pub inner_request: crate::domain::value_objects::r2ps::InnerRequest,
    pub session_id: Option<SessionId>,
    pub device_kid: String,
    pub session_state: Option<SessionState>,
}

pub struct OperationResult {
    pub state: Option<crate::domain::DeviceHsmState>,
    pub data: crate::domain::InnerResponseData,
    pub session_id: Option<SessionId>,
    pub session_transition: Option<SessionTransition>,
}

impl OperationResult {
    pub fn to_inner_response(
        &self,
        serialized_data: String,
        ttl: Option<std::time::Duration>,
        hsm_state_version: Option<u64>,
    ) -> crate::domain::value_objects::r2ps::InnerResponse {
        use crate::domain::value_objects::r2ps::{to_iso8601_duration, InnerResponse};

        InnerResponse::ok(
            serialized_data,
            ttl.map(to_iso8601_duration),
            hsm_state_version,
        )
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
    pin_change_start_op: PinChangeStartOperation,
    pin_change_finish_op: PinChangeFinishOperation,
    hsm_ecdsa_op: HsmSignOperation,
    hsm_keygen_op: HsmGenerateKeyOperation,
    hsm_delete_key_op: HsmDeleteKeyOperation,
    hsm_list_keys_op: HsmListKeysOperation,
    session_end_op: SessionEndOperation,
    state_init_op: StateInitOperation,
}

impl OperationDispatcher {
    pub fn from_dependencies(
        pake_port: Arc<dyn PakePort>,
        hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
    ) -> Self {
        Self {
            authenticate_start_op: AuthenticateStartOperation::new(pake_port.clone()),
            authenticate_finish_op: AuthenticateFinishOperation::new(pake_port.clone()),
            register_start_op: RegisterStartOperation::new(pake_port.clone()),
            register_finish_op: RegisterFinishOperation::new(pake_port.clone()),
            pin_change_start_op: PinChangeStartOperation::new(pake_port.clone()),
            pin_change_finish_op: PinChangeFinishOperation::new(pake_port),
            hsm_ecdsa_op: HsmSignOperation::new(hsm_spi_port.clone()),
            hsm_keygen_op: HsmGenerateKeyOperation::new(hsm_spi_port.clone()),
            hsm_delete_key_op: HsmDeleteKeyOperation,
            hsm_list_keys_op: HsmListKeysOperation,
            session_end_op: SessionEndOperation,
            state_init_op: StateInitOperation,
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
            OperationId::StateInit => self.state_init_op.execute(context),
            OperationId::AuthenticateStart => self.authenticate_start_op.execute(context),
            OperationId::AuthenticateFinish => self.authenticate_finish_op.execute(context),
            OperationId::RegisterStart => self.register_start_op.execute(context),
            OperationId::RegisterFinish => self.register_finish_op.execute(context),
            OperationId::ChangePinStart => self.pin_change_start_op.execute(context),
            OperationId::ChangePinFinish => self.pin_change_finish_op.execute(context),
            OperationId::HsmSign => self.hsm_ecdsa_op.execute(context),
            OperationId::HsmGenerateKey => self.hsm_keygen_op.execute(context),
            OperationId::HsmDeleteKey => self.hsm_delete_key_op.execute(context),
            OperationId::HsmListKeys => self.hsm_list_keys_op.execute(context),
            OperationId::EndSession => self.session_end_op.execute(context),
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
