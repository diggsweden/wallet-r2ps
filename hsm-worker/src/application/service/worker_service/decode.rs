// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::jose_port;
use crate::application::port::outgoing::session_state_spi_port::SessionKey;
use crate::application::protocol::OuterRequestExt;
use crate::application::service::operations::OperationContext;
use crate::application::service::worker_service::context::{ResponseContext, WorkerInput};
use crate::application::service::worker_service::error::{OuterError, UpstreamError, WorkerError};
use crate::domain::OuterRequest;
use crate::domain::{DeviceHsmState, EcPublicJwk, HsmWorkerRequest, SessionId};
use std::sync::Arc;
use tracing::info;

/// Intermediate result after decoding the outer JWS, before inner JWE decryption.
pub struct PartialRequest {
    pub request_id: String,
    pub state: DeviceHsmState,
    pub outer_request: OuterRequest,
    pub device_kid: String,
    pub device_public_key: EcPublicJwk,
}

pub struct RequestDecoder {
    jose: Arc<dyn jose_port::JosePort>,
    /// True in HSM mode: clients must include server_kid in every request.
    /// False in legacy mode: SERVER_PRIVATE_KEY is stable, so clients may omit it.
    kid_required: bool,
}

impl RequestDecoder {
    pub fn new(jose: Arc<dyn jose_port::JosePort>, legacy_key_mode: bool) -> Self {
        Self {
            jose,
            kid_required: !legacy_key_mode,
        }
    }

    /// Phase 1: Verify state JWS and outer request JWS, extract session_id.
    /// Pure — no side effects. Caller uses session_id to read session state.
    pub fn decode_outer(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<PartialRequest, WorkerError> {
        let HsmWorkerRequest {
            request_id,
            state_jws,
            outer_request_jws,
            ..
        } = hsm_worker_request;

        let state = DeviceHsmState::from_jws(state_jws.as_str(), self.jose.as_ref())
            .map_err(|_| UpstreamError::InvalidStateJws)?;

        let device_kid = self
            .jose
            .peek_kid(outer_request_jws.as_str())
            .ok_or(UpstreamError::OuterJwsMissingKid)?;

        let device_public_key = state
            .find_device_key(&device_kid)
            .ok_or(UpstreamError::UnknownDevice)?
            .public_key
            .clone();

        let outer_request = OuterRequest::from_jws(
            outer_request_jws.as_str(),
            self.jose.as_ref(),
            &device_public_key,
        )?;

        if outer_request.context != "hsm" {
            return Err(OuterError::UnsupportedContext.into());
        }

        match &outer_request.server_kid {
            Some(kid) if kid != self.jose.jws_kid() => {
                return Err(UpstreamError::UnknownServerKid.into());
            }
            None if self.kid_required => {
                return Err(UpstreamError::ServerKidRequired.into());
            }
            _ => {}
        }

        Ok(PartialRequest {
            request_id,
            state,
            outer_request,
            device_kid,
            device_public_key,
        })
    }

    /// Phase 2: Decrypt inner JWE using the provided session key (or device key).
    /// Pure — no side effects. session_id and session_state are provided by the caller.
    pub fn decode_inner(
        &self,
        partial: PartialRequest,
        session_id: Option<SessionId>,
        session_key: Option<&SessionKey>,
    ) -> Result<WorkerInput, WorkerError> {
        let inner_request = partial
            .outer_request
            .decrypt_inner(self.jose.as_ref(), session_key)?;

        info!(
            "Processing request id {} of type {:?}",
            partial.request_id, inner_request.request_type
        );

        let operation_context = OperationContext {
            request_id: partial.request_id.clone(),
            state: partial.state,
            outer_request: partial.outer_request,
            inner_request,
            session_id,
            device_kid: partial.device_kid,
            session_state: None, // populated by WorkerService after cache read
        };

        let response_context = ResponseContext {
            request_id: partial.request_id,
            request_type: operation_context.inner_request.request_type,
            session_key: None, // populated by WorkerService
            ttl: None,         // populated by WorkerService
            device_public_key: partial.device_public_key,
        };

        Ok(WorkerInput {
            operation_context,
            response_context,
        })
    }
}
