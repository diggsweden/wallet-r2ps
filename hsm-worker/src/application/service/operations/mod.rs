// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod authentication;
pub mod hsm;
pub mod session;

#[cfg(test)]
mod authentication_tests;
#[cfg(test)]
mod hsm_tests;

use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::pake_port::PakePort;
use crate::application::port::outgoing::session_state_spi_port::SessionState;
use crate::domain::{OperationId, ServiceRequestError, SessionId};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Clone, Debug)]
pub struct InnerResponseData {
    data: serde_json::Value,
}

impl InnerResponseData {
    pub fn new<T: Serialize>(data: T) -> Result<Self, ServiceRequestError> {
        serde_json::to_value(data)
            .map(|value| Self { data: value })
            .map_err(|_| ServiceRequestError::Unknown)
    }

    pub fn serialize(&self) -> Result<Vec<u8>, ServiceRequestError> {
        serde_json::to_vec(&self.data).map_err(|_| ServiceRequestError::Unknown)
    }
}

/// Converts a [`Duration`] to an ISO 8601 duration (seconds only).
pub fn to_iso8601_duration(d: Duration) -> iso8601_duration::Duration {
    iso8601_duration::Duration::new(0.0, 0.0, 0.0, 0.0, 0.0, d.as_secs() as f32)
}

pub use crate::application::port::outgoing::session_state_spi_port::SessionTransition;

use authentication::{
    AuthenticateFinishOperation, AuthenticateStartOperation, PinChangeFinishOperation,
    PinChangeStartOperation, RegisterFinishOperation, RegisterStartOperation,
};
use hsm::{HsmDeleteKeyOperation, HsmGenerateKeyOperation, HsmListKeysOperation, HsmSignOperation};
use session::SessionEndOperation;

#[derive(Debug)]
pub struct OperationContext {
    pub request_id: String,
    pub state: crate::domain::DeviceHsmState,
    pub outer_request: crate::domain::OuterRequest,
    pub inner_request: crate::domain::InnerRequest,
    pub session_id: Option<SessionId>,
    pub device_kid: String,
    pub session_state: Option<SessionState>,
}

pub struct OperationResult {
    pub state: Option<crate::domain::DeviceHsmState>,
    pub data: InnerResponseData,
    pub session_id: Option<SessionId>,
    pub session_transition: Option<SessionTransition>,
}

impl OperationResult {
    /// Creates an InnerResponse from this OperationResult with the serialized response data
    pub fn to_inner_response(
        &self,
        serialized_data: String,
        ttl: Option<Duration>,
    ) -> crate::domain::InnerResponse {
        crate::domain::InnerResponse::ok(serialized_data, ttl.map(to_iso8601_duration))
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
}

impl OperationDispatcher {
    /// Creates a new OperationDispatcher with all operation handlers initialized
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
