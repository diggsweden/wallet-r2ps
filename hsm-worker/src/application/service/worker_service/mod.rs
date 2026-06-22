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
    pub fn new(ports: WorkerPorts, hsm_key_label: String, legacy_key_mode: bool) -> Self {
        let operation_dispatcher =
            OperationDispatcher::from_dependencies(ports.pake, ports.hsm, hsm_key_label);

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

/// Wall-clock breakdown of one request through `process_request`. Each
/// field measures the elapsed wall time of one phase in microseconds so
/// the worker-side log line says exactly where the 91–353 ms `execute_us`
/// from the per-pod stats actually went.
#[derive(Debug, Default)]
struct PhaseTimings {
    decode_outer_us: u128,
    state_read_us: u128,
    decode_inner_us: u128,
    dispatch_us: u128,
    apply_transition_us: u128,
    ttl_us: u128,
    encode_us: u128,
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
        let response_topic = hsm_worker_request.response_topic.clone();

        let (response, phase_timings) = match self.process_request(hsm_worker_request) {
            Ok((res, ts)) => (res, ts),
            Err(process_err) => {
                error!("Request {} failed: {:?}", request_id, process_err.error);
                match self
                    .response_builder
                    .build_error_response(&request_id, process_err)
                {
                    Ok(response) => (response, PhaseTimings::default()),
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

        let t_send = Instant::now();
        self.worker_response_spi_port
            .send(response, &response_topic)
            .map_err(|_| WorkerRequestError::ConnectionError)?;
        let send_us = t_send.elapsed().as_micros();

        info!(
            request_id = %request_id,
            total_ms = start.elapsed().as_millis() as u64,
            decode_outer_us = phase_timings.decode_outer_us as u64,
            state_read_us = phase_timings.state_read_us as u64,
            decode_inner_us = phase_timings.decode_inner_us as u64,
            dispatch_us = phase_timings.dispatch_us as u64,
            apply_transition_us = phase_timings.apply_transition_us as u64,
            ttl_us = phase_timings.ttl_us as u64,
            encode_us = phase_timings.encode_us as u64,
            send_us = send_us as u64,
            "request_phases"
        );

        Ok(request_id)
    }
}

impl WorkerService {
    /// The core execution pipeline: Decode → Read state → Dispatch → Apply transition → Encode.
    fn process_request(
        &self,
        request: HsmWorkerRequest,
    ) -> Result<(HsmWorkerResponse, PhaseTimings), ProcessError> {
        let mut ts = PhaseTimings::default();

        // Phase 1: Decode outer (pure — no side effects)
        let t = Instant::now();
        let partial = self
            .request_decoder
            .decode_outer(request)
            .map_err(|error| ProcessError {
                error,
                context: None,
            })?;
        ts.decode_outer_us = t.elapsed().as_micros();

        let session_id = partial.outer_request.session_id.clone();

        // Phase 2: Read session state from cache
        let t = Instant::now();
        let session_state = session_id
            .as_ref()
            .and_then(|id| self.session_state_port.get(id));
        ts.state_read_us = t.elapsed().as_micros();

        let session_key_for_response = match &session_state {
            Some(SessionState::Active(data)) => Some(data.session_key.clone()),
            _ => None,
        };

        // Phase 3: Decode inner (pure — no side effects)
        let t = Instant::now();
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
        ts.decode_inner_us = t.elapsed().as_micros();

        operation_context.session_state = session_state;
        let response_context = ResponseContext {
            session_key: session_key_for_response.clone(),
            ..response_context
        };

        // Phase 4: Dispatch (pure — no side effects)
        let t = Instant::now();
        let operation_result = self
            .operation_dispatcher
            .dispatch(operation_context)
            .map_err(|err| ProcessError {
                error: WorkerError::Inner(err),
                context: Some(Box::new(response_context.clone())),
            })?;
        ts.dispatch_us = t.elapsed().as_micros();

        // Phase 5: Apply session state transition
        let t = Instant::now();
        self.session_state_port
            .apply_transition(
                operation_result.session_id.as_ref(),
                operation_result.session_transition.as_ref(),
            )
            .map_err(|_| ProcessError {
                error: WorkerError::Inner(crate::domain::ServiceRequestError::InternalServerError),
                context: Some(Box::new(response_context.clone())),
            })?;
        ts.apply_transition_us = t.elapsed().as_micros();

        // Phase 6: Compute TTL from post-transition state
        let t = Instant::now();
        let ttl = self
            .session_state_port
            .get_remaining_ttl(operation_result.session_id.as_ref());
        ts.ttl_us = t.elapsed().as_micros();

        // Phase 7: Encode response (pure — no side effects)
        let full_response_context = ResponseContext {
            ttl,
            ..response_context
        };

        let t = Instant::now();
        let response = self
            .response_builder
            .encode_response(operation_result, &full_response_context)
            .map_err(|error| ProcessError {
                error,
                context: Some(Box::new(full_response_context)),
            })?;
        ts.encode_us = t.elapsed().as_micros();

        Ok((response, ts))
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
    use rand_core::OsRng;

    let secret_key = SecretKey::random(&mut OsRng);
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
