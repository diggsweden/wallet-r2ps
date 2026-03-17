pub mod context;
pub mod decode;
pub mod error;
pub mod response;

#[cfg(test)]
mod tests;

pub use context::{ResponseContext, WorkerInput};
pub use error::{OuterError, ProblemDetail, UpstreamError, WorkerError};

use crate::application::port::outgoing::jose_port;
use crate::application::port::outgoing::response_publisher_port::ResponsePublisher;
use crate::application::port::outgoing::session_state_spi_port::SessionStateSpiPort;
use crate::application::port::outgoing::state_cache_port::{StateCache, TamperDetectionCache};
use crate::application::port::outgoing::state_repository_port::{
    OutboxEntry, StateError, StateRepository,
};
use crate::application::service::operations::OperationDispatcher;
use crate::application::{WorkerPorts, WorkerRequestId, WorkerRequestUseCase};
use crate::domain::value_objects::r2ps::{StateVersionEvent, Status};
use crate::domain::{
    DeviceHsmState, HsmWorkerRequest, OperationId, StateInitCommandDto, WorkerRequestError,
    WorkerResponse,
};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

use decode::RequestDecoder;
use response::{ProcessError, ResponseBuilder};

/// Orchestrates the processing of requests from Kafka.
/// A stateful command processor with DB-backed state, transactional outbox,
/// and session FSM for OPAQUE lifecycle management.
pub struct WorkerService {
    jose: Arc<dyn jose_port::JosePort>,
    state_repository: Arc<dyn StateRepository>,
    response_publisher: Arc<dyn ResponsePublisher>,
    tamper_cache: Arc<dyn TamperDetectionCache>,
    state_cache: Arc<dyn StateCache>,
    session_state_port: Arc<dyn SessionStateSpiPort>,
    operation_dispatcher: OperationDispatcher,
    request_decoder: RequestDecoder,
    response_builder: ResponseBuilder,
}

impl WorkerService {
    pub fn new(jose: Arc<dyn jose_port::JosePort>, ports: WorkerPorts) -> Self {
        let operation_dispatcher = OperationDispatcher::from_dependencies(ports.pake, ports.hsm);

        let request_decoder = RequestDecoder::new(jose.clone(), ports.session_state.clone());
        let response_builder = ResponseBuilder::new(jose.clone(), ports.session_state.clone());

        Self {
            jose,
            state_repository: ports.state_repository,
            response_publisher: ports.response_publisher,
            tamper_cache: ports.tamper_cache,
            state_cache: ports.state_cache,
            session_state_port: ports.session_state,
            operation_dispatcher,
            request_decoder,
            response_builder,
        }
    }

    /// Execute a StateInit command (plaintext from BFF, no JWS envelope).
    pub fn execute_state_init(
        &self,
        command: StateInitCommandDto,
    ) -> Result<WorkerRequestId, WorkerRequestError> {
        let start = Instant::now();
        let correlation_id = command.correlation_id.clone();
        let device_id = command.device_id.clone();

        info!(
            "Processing state-init for device_id={}, correlation_id={}",
            device_id, correlation_id
        );

        match self.process_state_init_command(&command) {
            Ok(()) => {
                info!(
                    "State-init complete for device_id={} (took {} ms)",
                    device_id,
                    start.elapsed().as_millis()
                );
                Ok(correlation_id)
            }
            Err(err) => {
                error!("State-init failed for device_id={}: {:?}", device_id, err);
                self.write_error_response(&correlation_id, &device_id, None, &err);
                Err(err)
            }
        }
    }

    /// Process a state-init command: creates version 0 state, persists via outbox.
    fn process_state_init_command(
        &self,
        command: &StateInitCommandDto,
    ) -> Result<(), WorkerRequestError> {
        use crate::application::service::operations::state_init::StateInitOperation;
        use crate::application::service::operations::{OperationContext, ServiceOperation};
        use crate::domain::value_objects::r2ps::{InnerRequest, OuterRequest};

        // Build a synthetic OperationContext for the state-init operation
        let inner_request = InnerRequest {
            version: 1,
            request_type: OperationId::StateInit,
            request_counter: 0,
            data: Some(
                serde_json::to_string(&crate::domain::StateInitInnerRequest {
                    public_key: command.public_key.clone(),
                })
                .map_err(|e| WorkerRequestError::DatabaseError(e.to_string()))?,
            ),
        };

        let dummy_state = DeviceHsmState {
            version: 0,
            device_keys: vec![],
            hsm_keys: vec![],
        };

        let context = OperationContext {
            correlation_id: command.correlation_id.clone(),
            device_id: command.device_id.clone(),
            state: dummy_state,
            outer_request: OuterRequest {
                version: 1,
                session_id: None,
                context: "state-init".to_string(),
                inner_jwe: None,
            },
            inner_request,
            session_id: None,
            device_kid: command.public_key.kid.clone(),
            session_state: None,
        };

        let op = StateInitOperation;
        let operation_result = op.execute(context).map_err(|e| {
            error!("StateInit operation failed: {:?}", e);
            WorkerRequestError::DatabaseError(format!("operation failed: {:?}", e))
        })?;

        let new_state = operation_result
            .state
            .clone()
            .expect("StateInit must produce a state");

        // Build a ResponseContext so we can use the standard ResponseBuilder
        let response_context = ResponseContext {
            correlation_id: command.correlation_id.clone(),
            device_id: command.device_id.clone(),
            request_id: None,
            request_type: OperationId::StateInit,
            session_key: None,
            device_public_key: command.public_key.clone(),
        };

        // Build proper JWS/JWE response via ResponseBuilder (same as other mutations)
        let response = self
            .response_builder
            .encode_response(operation_result, &response_context, Some(0))
            .map_err(|_| WorkerRequestError::ResponseBuildError)?;

        // Sign and persist state
        let state_jws = self.sign_state(&new_state)?;

        let outbox_entries = self.build_outbox_entries(
            &command.device_id,
            &command.correlation_id,
            &response,
            &new_state,
            &state_jws,
            "StateInit",
        )?;

        self.state_repository
            .save_state_with_outbox(
                &command.device_id,
                None,
                0,
                &state_jws,
                "StateInit",
                &command.correlation_id,
                outbox_entries,
            )
            .map_err(|e| match e {
                StateError::ClientAlreadyExists => {
                    warn!("Device {} already exists", command.device_id);
                    WorkerRequestError::ConcurrencyConflict
                }
                StateError::ConcurrencyConflict => WorkerRequestError::ConcurrencyConflict,
                other => WorkerRequestError::DatabaseError(format!("{}", other)),
            })?;

        // Update caches
        self.state_cache.put(&command.device_id, new_state.clone());
        self.tamper_cache.put(&command.device_id, 0);

        info!("State-init persisted for device_id={}", command.device_id);
        Ok(())
    }
}

impl WorkerRequestUseCase for WorkerService {
    fn execute_state_init(
        &self,
        command: StateInitCommandDto,
    ) -> Result<WorkerRequestId, WorkerRequestError> {
        self.execute_state_init(command)
    }

    fn execute(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerRequestId, WorkerRequestError> {
        let start = Instant::now();
        let correlation_id = hsm_worker_request.correlation_id.clone();
        let device_id = hsm_worker_request.device_id.clone();
        let request_id = hsm_worker_request.request_id.clone();

        match self.process_command(hsm_worker_request) {
            Ok(()) => {
                info!(
                    "Processed correlation_id {} (took {} ms)",
                    correlation_id,
                    start.elapsed().as_millis(),
                );
                Ok(correlation_id)
            }
            Err(process_err) => {
                error!("Request {} failed: {:?}", correlation_id, process_err.error);
                match self.response_builder.build_error_response(
                    &correlation_id,
                    &device_id,
                    request_id.as_deref(),
                    process_err,
                ) {
                    Ok(response) => {
                        self.publish_response_directly(&response);
                        Ok(correlation_id)
                    }
                    Err(build_err) => {
                        error!(
                            "Request {} failed to build error response: {:?}",
                            correlation_id, build_err
                        );
                        Err(build_err)
                    }
                }
            }
        }
    }
}

impl WorkerService {
    /// The core command processing pipeline.
    fn process_command(&self, request: HsmWorkerRequest) -> Result<(), ProcessError> {
        let device_id = request.device_id.clone();
        let correlation_id = request.correlation_id.clone();
        let request_id = request.request_id.clone();

        // 1. Load state
        let state = self.load_state_for_command(&device_id)?;

        // Use client-supplied state version if present, otherwise fall back to loaded state version.
        // This ensures mutations always go through the UPDATE path in save_state_with_outbox,
        // even when the BFF doesn't supply a state version (REST clients).
        let state_version = request.state_version.or(Some(state.version));

        // 2. Decode (verify JWS, decrypt JWE, look up session state)
        let WorkerInput {
            operation_context,
            response_context,
        } = self
            .request_decoder
            .decode_request(request, state)
            .map_err(|error| ProcessError {
                error,
                context: None,
            })?;

        let op_type = operation_context.inner_request.request_type;
        let mutates = op_type.mutates_state();

        // 3. Dispatch operation
        // Note: tamper detection already performed in load_state_for_command()
        // for all DB loads (both read-only and mutating operations).
        let operation_result = self
            .operation_dispatcher
            .dispatch(operation_context)
            .map_err(|err| ProcessError {
                error: WorkerError::Inner(err),
                context: Some(Box::new(response_context.clone())),
            })?;

        // 4. Apply session state transition (FSM)
        self.session_state_port
            .apply_transition(
                operation_result.session_id.as_ref(),
                operation_result.session_transition.as_ref(),
            )
            .map_err(|_| ProcessError {
                error: WorkerError::Inner(crate::domain::ServiceRequestError::InternalServerError),
                context: Some(Box::new(response_context.clone())),
            })?;

        // 5. Persist and respond
        if mutates && operation_result.state.is_some() {
            self.persist_and_respond(
                &device_id,
                &correlation_id,
                request_id.as_deref(),
                state_version,
                &response_context,
                operation_result,
            )
            .map_err(|error| ProcessError {
                error,
                context: Some(Box::new(response_context.clone())),
            })?;
        } else {
            // Read-only: build response and publish directly
            let response = self
                .response_builder
                .encode_response(operation_result, &response_context, None)
                .map_err(|error| ProcessError {
                    error,
                    context: Some(Box::new(response_context.clone())),
                })?;
            self.publish_response_directly(&response);
        }

        Ok(())
    }

    /// Load state for a command. Try in-memory cache first, then DB.
    ///
    /// # Security guarantees
    ///
    /// - **Cache hit:** Returns immediately. The state was already JWS-verified
    ///   and tamper-checked when it was first loaded from DB and placed in cache.
    /// - **Cache miss (DB load):** The state JWS signature is cryptographically
    ///   verified (ES256), then tamper detection compares the DB version against
    ///   the redb monotonic high-water mark to detect rollback attacks. Only
    ///   states that pass both checks are cached and returned.
    ///
    /// This ensures that **all** operations (read-only and mutating) operate on
    /// verified, tamper-checked state when loaded from the database.
    fn load_state_for_command(&self, device_id: &str) -> Result<DeviceHsmState, ProcessError> {
        // Fast path: cache hit (already JWS-verified and tamper-checked on initial load)
        if let Some(cached) = self.state_cache.get(device_id) {
            return Ok(cached);
        }

        // Slow path: load from PostgreSQL
        let versioned = self
            .state_repository
            .load_current_state(device_id)
            .map_err(|_e| ProcessError {
                error: WorkerError::Upstream(UpstreamError::InvalidStateJws),
                context: None,
            })?;

        match versioned {
            Some(vs) => {
                // 1. Cryptographic verification: validate ES256 JWS signature
                let state = self
                    .verify_state_jws(&vs.state_jws)
                    .map_err(|_| ProcessError {
                        error: WorkerError::Upstream(UpstreamError::InvalidStateJws),
                        context: None,
                    })?;

                // 2. Tamper detection: reject if DB version < redb high-water mark
                self.check_tamper(device_id, &state)
                    .map_err(|error| ProcessError {
                        error,
                        context: None,
                    })?;

                // 3. Cache the verified + tamper-checked state
                self.state_cache.put(device_id, state.clone());
                Ok(state)
            }
            None => Err(ProcessError {
                error: WorkerError::Inner(crate::domain::ServiceRequestError::StateNotFound),
                context: None,
            }),
        }
    }

    fn verify_state_jws(&self, jws: &str) -> Result<DeviceHsmState, ()> {
        DeviceHsmState::from_jws(jws, self.jose.as_ref()).map_err(|_| ())
    }

    /// Tamper detection: compare loaded state version against redb monotonic
    /// high-water mark. Detects database rollback attacks where an attacker
    /// replaces the current state with an older version.
    ///
    /// Called from [`load_state_for_command`] on every database load (both
    /// read-only and mutating operations). Cache hits skip this check because
    /// they were already tamper-checked when first loaded from the database.
    fn check_tamper(&self, device_id: &str, state: &DeviceHsmState) -> Result<(), WorkerError> {
        if let Some(cached_version) = self.tamper_cache.get(device_id) {
            if state.version < cached_version {
                error!(
                    "Tamper detected for device {}: DB version {} < cache version {}",
                    device_id, state.version, cached_version
                );
                return Err(WorkerError::Inner(
                    crate::domain::ServiceRequestError::TamperDetected,
                ));
            }
        }
        Ok(())
    }

    /// Persist state mutation atomically via outbox and build response.
    fn persist_and_respond(
        &self,
        device_id: &str,
        correlation_id: &str,
        _request_id: Option<&str>,
        expected_version: Option<u64>,
        response_context: &ResponseContext,
        operation_result: crate::application::service::operations::OperationResult,
    ) -> Result<(), WorkerError> {
        // Extract new_state before moving operation_result
        let mut new_state = operation_result
            .state
            .clone()
            .expect("mutating operation must produce state");

        let new_version = new_state.version + 1;

        // Update version before signing — ensures state JWS, hash,
        // and all caches (Moka, redb) agree on the version number.
        new_state.version = new_version;

        let state_jws = self.sign_state(&new_state).map_err(|_e| {
            WorkerError::Upstream(UpstreamError::EncodeFailed("state_sign_failed"))
        })?;
        let command_type = format!("{:?}", response_context.request_type);

        // Build the response (moves operation_result)
        let response = self.response_builder.encode_response(
            operation_result,
            response_context,
            Some(new_version),
        )?;

        // Build outbox entries
        let outbox_entries = self
            .build_outbox_entries(
                device_id,
                correlation_id,
                &response,
                &new_state,
                &state_jws,
                &command_type,
            )
            .map_err(|_| {
                WorkerError::Upstream(UpstreamError::EncodeFailed("outbox_build_failed"))
            })?;

        // Atomically persist state + outbox entries
        self.state_repository
            .save_state_with_outbox(
                device_id,
                expected_version,
                new_version,
                &state_jws,
                &command_type,
                correlation_id,
                outbox_entries,
            )
            .map_err(|e| match e {
                StateError::ConcurrencyConflict => {
                    WorkerError::Inner(crate::domain::ServiceRequestError::ConcurrencyConflict)
                }
                StateError::ClientAlreadyExists => {
                    WorkerError::Inner(crate::domain::ServiceRequestError::ClientAlreadyExists)
                }
                _other => WorkerError::Upstream(UpstreamError::EncodeFailed("db_persist_failed")),
            })?;

        // Update caches
        self.state_cache.put(device_id, new_state);
        self.tamper_cache.put(device_id, new_version);

        Ok(())
    }

    /// Build outbox entries for r2ps-responses, state-versions, and state-snapshot topics.
    fn build_outbox_entries(
        &self,
        device_id: &str,
        correlation_id: &str,
        response: &WorkerResponse,
        new_state: &DeviceHsmState,
        state_jws: &str,
        command_type: &str,
    ) -> Result<Vec<OutboxEntry>, WorkerRequestError> {
        let mut entries = Vec::with_capacity(3);

        // 1. Response event -> r2ps-responses
        let response_payload =
            serde_json::to_value(response).map_err(|_| WorkerRequestError::ResponseBuildError)?;
        entries.push(OutboxEntry {
            topic: "r2ps-responses".to_string(),
            key: device_id.to_string(),
            payload: response_payload,
        });

        // 2. State version event -> state-versions (audit)
        let version_event = StateVersionEvent {
            device_id: device_id.to_string(),
            version: new_state.version,
            command_type: command_type.to_string(),
            correlation_id: correlation_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let version_payload = serde_json::to_value(&version_event)
            .map_err(|_| WorkerRequestError::ResponseBuildError)?;
        entries.push(OutboxEntry {
            topic: "state-versions".to_string(),
            key: device_id.to_string(),
            payload: version_payload,
        });

        // 3. State snapshot -> state-snapshot (log-compacted)
        let snapshot_payload = serde_json::json!({
            "device_id": device_id,
            "state_jws": state_jws,
            "version": new_state.version,
        });
        entries.push(OutboxEntry {
            topic: "state-snapshot".to_string(),
            key: device_id.to_string(),
            payload: snapshot_payload,
        });

        Ok(entries)
    }

    fn sign_state(&self, state: &DeviceHsmState) -> Result<String, WorkerRequestError> {
        let bytes = state
            .serialize()
            .map_err(|_| WorkerRequestError::ResponseBuildError)?;
        self.jose
            .jws_sign(&bytes)
            .map_err(|_| WorkerRequestError::ResponseBuildError)
    }

    /// Publish a response directly to Kafka (bypassing outbox).
    fn publish_response_directly(&self, response: &WorkerResponse) {
        let payload = match serde_json::to_vec(response) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to serialize response: {:?}", e);
                return;
            }
        };
        if let Err(e) = self
            .response_publisher
            .publish_response(&response.device_id, &payload)
        {
            error!("Failed to publish response directly: {}", e);
        }
    }

    fn write_error_response(
        &self,
        correlation_id: &str,
        device_id: &str,
        request_id: Option<&str>,
        err: &WorkerRequestError,
    ) {
        let error_msg = format!("{:?}", err);
        let response = WorkerResponse {
            correlation_id: correlation_id.to_string(),
            device_id: device_id.to_string(),
            request_id: request_id.map(|s| s.to_string()),
            outer_response_jws: None,
            status: Status::Error,
            error_message: Some(error_msg),
        };
        self.publish_response_directly(&response);
    }
}
