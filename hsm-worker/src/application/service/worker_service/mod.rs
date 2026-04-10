// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod context;
pub mod decode;
pub mod error;
pub mod response;

#[cfg(test)]
mod decode_tests;
#[cfg(test)]
mod response_tests;
#[cfg(test)]
pub(crate) mod test_utils;
#[cfg(test)]
mod tests;

pub use context::{ResponseContext, WorkerInput};
pub use error::{OuterError, ProblemDetail, UpstreamError, WorkerError};

use crate::application::port::incoming::worker_request_use_case::WorkerRequestError;
use crate::application::port::outgoing::session_state_spi_port::{
    SessionState, SessionStateSpiPort,
};
use crate::application::service::operations::OperationDispatcher;
use crate::application::{
    WorkerPorts, WorkerRequestId, WorkerRequestUseCase, WorkerResponseSpiPort,
};
use crate::domain::{HsmWorkerRequest, HsmWorkerResponse};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

#[cfg(test)]
use crate::application::jose_port::JosePort;
use decode::RequestDecoder;
use response::{ProcessError, ResponseBuilder};

/// Orchestrates the processing of requests from Kafka.
pub struct WorkerService {
    worker_response_spi_port: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    operation_dispatcher: OperationDispatcher,
    request_decoder: RequestDecoder,
    response_builder: ResponseBuilder,
    session_state_port: Arc<dyn SessionStateSpiPort>,
}

impl WorkerService {
    pub fn new(ports: WorkerPorts, legacy_key_mode: bool) -> Self {
        let operation_dispatcher = OperationDispatcher::from_dependencies(ports.pake, ports.hsm);

        let request_decoder = RequestDecoder::new(ports.jose.clone(), legacy_key_mode);
        let response_builder = ResponseBuilder::new(ports.jose);

        Self {
            worker_response_spi_port: ports.worker_response,
            operation_dispatcher,
            request_decoder,
            response_builder,
            session_state_port: ports.session_state,
        }
    }
}

impl WorkerRequestUseCase for WorkerService {
    /// The entry point for processing a single request from Kafka.
    /// It handles the end-to-end execution and all error reporting back to the sender.
    fn execute(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerRequestId, WorkerRequestError> {
        let start = Instant::now();
        let request_id = hsm_worker_request.request_id.clone();

        let response = match self.process_request(hsm_worker_request) {
            Ok(res) => res,
            Err(process_err) => {
                error!("Request {} failed: {:?}", request_id, process_err.error);
                match self
                    .response_builder
                    .build_error_response(&request_id, process_err)
                {
                    Ok(response) => response,
                    Err(build_err) => {
                        error!(
                            "Request {} failed to build error response: {:?}",
                            request_id, build_err
                        );
                        return Err(build_err);
                    }
                }
            }
        };

        self.worker_response_spi_port
            .send(response)
            .map_err(|_| WorkerRequestError::ConnectionError)?;

        info!(
            "Processed request id {} (took {} ms)",
            request_id,
            start.elapsed().as_millis(),
        );

        Ok(request_id)
    }
}

impl WorkerService {
    /// The core execution pipeline: Decode → Read state → Dispatch → Apply transition → Encode.
    fn process_request(
        &self,
        request: HsmWorkerRequest,
    ) -> Result<HsmWorkerResponse, ProcessError> {
        // Phase 1: Decode outer (pure — no side effects)
        let partial = self
            .request_decoder
            .decode_outer(request)
            .map_err(|error| ProcessError {
                error,
                context: None,
            })?;

        let session_id = partial.outer_request.session_id.clone();

        // Phase 2: Read session state from cache
        let session_state = session_id
            .as_ref()
            .and_then(|id| self.session_state_port.get(id));

        let session_key_for_response = match &session_state {
            Some(SessionState::Active(data)) => Some(data.session_key.clone()),
            _ => None,
        };

        // Phase 3: Decode inner (pure — no side effects)
        let WorkerInput {
            mut operation_context,
            response_context,
        } = self
            .request_decoder
            .decode_inner(
                partial,
                session_id.clone(),
                session_key_for_response.as_ref(),
            )
            .map_err(|error| ProcessError {
                error,
                context: None,
            })?;

        operation_context.session_state = session_state;
        let response_context = ResponseContext {
            session_key: session_key_for_response.clone(),
            ..response_context
        };

        // Phase 4: Dispatch (pure — no side effects)
        let operation_result = self
            .operation_dispatcher
            .dispatch(operation_context)
            .map_err(|err| ProcessError {
                error: WorkerError::Inner(err),
                context: Some(Box::new(response_context.clone())),
            })?;

        // Phase 5: Apply session state transition
        self.session_state_port
            .apply_transition(
                operation_result.session_id.as_ref(),
                operation_result.session_transition.as_ref(),
            )
            .map_err(|_| ProcessError {
                error: WorkerError::Inner(crate::domain::ServiceRequestError::InternalServerError),
                context: Some(Box::new(response_context.clone())),
            })?;

        // Phase 6: Compute TTL from post-transition state
        let ttl = self
            .session_state_port
            .get_remaining_ttl(operation_result.session_id.as_ref());

        // Phase 7: Encode response (pure — no side effects)
        let full_response_context = ResponseContext {
            ttl,
            ..response_context
        };

        self.response_builder
            .encode_response(operation_result, &full_response_context)
            .map_err(|error| ProcessError {
                error,
                context: Some(Box::new(full_response_context)),
            })
    }
}

#[cfg(test)]
fn setup_crypto() -> (
    Arc<dyn JosePort>,
    josekit::jws::alg::ecdsa::EcdsaJwsVerifier,
) {
    use crate::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
    use p256::SecretKey;
    use p256::pkcs8::EncodePublicKey;

    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let public_key_pem = secret_key
        .public_key()
        .to_public_key_pem(Default::default())
        .unwrap();
    let jose = Arc::new(JoseAdapter::new(secret_key).unwrap());
    let verifier = josekit::jws::ES256
        .verifier_from_pem(public_key_pem.as_bytes())
        .unwrap();
    (jose, verifier)
}
