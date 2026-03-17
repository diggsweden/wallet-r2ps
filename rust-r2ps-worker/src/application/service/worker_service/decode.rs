use crate::application::jose_port;
use crate::application::port::outgoing::session_state_spi_port::{
    SessionState, SessionStateSpiPort,
};
use crate::application::service::operations::OperationContext;
use crate::application::service::worker_service::context::{ResponseContext, WorkerInput};
use crate::application::service::worker_service::error::{OuterError, UpstreamError, WorkerError};
use crate::domain::value_objects::r2ps::OuterRequest;
use crate::domain::{DeviceHsmState, EcPublicJwk, HsmWorkerRequest};
use std::sync::Arc;
use tracing::info;

/// Handles the decoding of HsmWorkerRequests.
/// State is now loaded externally (from DB or cache) and passed in.
pub struct RequestDecoder {
    jose: Arc<dyn jose_port::JosePort>,
    session_state_port: Arc<dyn SessionStateSpiPort>,
}

impl RequestDecoder {
    pub fn new(
        jose: Arc<dyn jose_port::JosePort>,
        session_state_port: Arc<dyn SessionStateSpiPort>,
    ) -> Self {
        Self {
            jose,
            session_state_port,
        }
    }

    /// Decodes an `HsmWorkerRequest` into its validated and decrypted parts.
    /// State is provided externally (loaded from DB or cache).
    pub fn decode_request(
        &self,
        hsm_worker_request: HsmWorkerRequest,
        state: DeviceHsmState,
    ) -> Result<WorkerInput, WorkerError> {
        let HsmWorkerRequest {
            correlation_id,
            device_id,
            request_id,
            outer_request_jws,
            state_version: _,
        } = hsm_worker_request;

        let (device_kid, device_public_key, outer_request) =
            self.decode_outer_request(outer_request_jws.as_str(), &state)?;

        let session_id = outer_request.session_id.clone();

        // Look up session state from FSM cache and extract session key
        let session_state = session_id
            .as_ref()
            .and_then(|id| self.session_state_port.get(id));

        let session_key = match &session_state {
            Some(SessionState::Active(data)) => Some(data.session_key.clone()),
            _ => None,
        };

        let inner_request =
            outer_request.decrypt_inner(self.jose.as_ref(), session_key.as_ref())?;

        info!(
            "Processing correlation_id {} of type {:?}",
            correlation_id, inner_request.request_type
        );

        let operation_context = OperationContext {
            correlation_id: correlation_id.clone(),
            device_id: device_id.clone(),
            state,
            outer_request,
            inner_request,
            session_id,
            device_kid,
            session_state,
        };

        let response_context = ResponseContext {
            correlation_id,
            device_id,
            request_id,
            request_type: operation_context.inner_request.request_type,
            session_key,
            device_public_key,
        };

        Ok(WorkerInput {
            operation_context,
            response_context,
        })
    }

    fn decode_outer_request(
        &self,
        outer_request_jws: &str,
        state: &DeviceHsmState,
    ) -> Result<(String, EcPublicJwk, OuterRequest), WorkerError> {
        let device_kid = self
            .jose
            .peek_kid(outer_request_jws)
            .map_err(|_| UpstreamError::OuterJwsInvalid)?
            .ok_or(UpstreamError::OuterJwsMissingKid)?;

        let device_public_key = state
            .find_device_key(&device_kid)
            .ok_or(UpstreamError::UnknownDevice)?
            .public_key
            .clone();

        let outer_request =
            OuterRequest::from_jws(outer_request_jws, self.jose.as_ref(), &device_public_key)?;

        if outer_request.context != "hsm" && outer_request.context != "state-init" {
            return Err(OuterError::UnsupportedContext.into());
        }

        Ok((device_kid, device_public_key, outer_request))
    }
}
