use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};

use crate::application::{
    WorkerPorts, WorkerRequestId, WorkerRequestUseCase, WorkerResponseSpiPort,
};
use crate::define_byte_vector;
use crate::domain::value_objects::r2ps::OuterRequest;
use crate::domain::{
    DeviceHsmState, EncryptOption, HsmWorkerRequest, OperationId, OuterResponse, TypedJwe,
    WorkerRequestError, WorkerResponse, WorkerServerConfig,
};
use josekit::jws::alg::ecdsa::{EcdsaJwsSigner, EcdsaJwsVerifier};
use pem::Pem;
use std::sync::Arc;
use std::time::Instant;
use josekit::jwk::Jwk;
use tracing::{debug, info};

define_byte_vector!(DecryptedData);

use super::opaque_factory::init_server_setup;
use super::operations::{OperationContext, OperationDispatcher, OperationResult};
use crate::application::OpaqueConfig;

pub struct WorkerService {
    worker_response_spi_port: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    worker_server_config: WorkerServerConfig,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    // Operation dispatcher
    operation_dispatcher: OperationDispatcher,
    jws_signer: Arc<EcdsaJwsSigner>,
    state_jws_verifier: EcdsaJwsVerifier,
}

impl WorkerService {
    pub fn new(
        server_public_key: Pem,
        server_private_key: Pem,
        jws_signer: Arc<EcdsaJwsSigner>,
        state_jws_verifier: EcdsaJwsVerifier,
        ports: WorkerPorts,
        opaque_config: OpaqueConfig,
    ) -> Self {
        let server_setup =
            init_server_setup(&opaque_config.opaque_server_setup, &server_private_key);

        let operation_dispatcher = OperationDispatcher::from_dependencies(
            server_setup,
            ports.session_key.clone(),
            ports.hsm,
            ports.pending_auth,
            opaque_config.opaque_context,
            opaque_config.opaque_server_identifier,
        );

        Self {
            worker_response_spi_port: ports.worker_response,
            worker_server_config: WorkerServerConfig {
                server_public_key,
                server_private_key,
            },
            operation_dispatcher,
            session_key_spi_port: ports.session_key,
            jws_signer,
            state_jws_verifier,
        }
    }

    /// Returns a reference to the server configuration
    pub fn server_config(&self) -> &WorkerServerConfig {
        &self.worker_server_config
    }
}

impl WorkerRequestUseCase for WorkerService {
    fn execute(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerRequestId, WorkerRequestError> {
        let start = Instant::now();
        let WorkerInput {
            operation_context,
            response_context,
        } = self.input(hsm_worker_request)?;

        let request_id = response_context.request_id.clone();
        let request_type = response_context.request_type;

        let operation_result = self
            .operation_dispatcher
            .dispatch(operation_context)
            .map_err(WorkerRequestError::ServiceError)?;

        let worker_response_jws = self.output(operation_result, response_context)?;

        let processing_elapsed = start.elapsed();
        debug!(
            "Request {:?} total processing time: {} ms",
            request_type,
            processing_elapsed.as_millis()
        );

        self.worker_response_spi_port
            .send(worker_response_jws)
            .map_err(|_| WorkerRequestError::ConnectionError)?;

        let finished_elapsed = start.elapsed();

        info!(
            "Responding to request id {} ({:?}, took {}/{} ms)",
            request_id,
            request_type,
            processing_elapsed.as_millis(),
            finished_elapsed.as_millis()
        );

        Ok(request_id)
    }
}

struct ResponseContext {
    request_id: String,
    request_type: OperationId,
    session_key: Option<SessionKey>,
    device_public_key: Jwk,
}

struct WorkerInput {
    operation_context: OperationContext,
    response_context: ResponseContext,
}

impl WorkerService {
    fn input(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerInput, WorkerRequestError> {
        let HsmWorkerRequest {
            request_id,
            state_jws,
            outer_request_jws,
        } = hsm_worker_request;

        let state = DeviceHsmState::decode_from_jws(state_jws.as_str(), &self.state_jws_verifier)
            .map_err(|_| WorkerRequestError::InvalidState)?;

        // Extract client public key kid from the JWS header
        let device_kid = OuterRequest::peek_kid(outer_request_jws.as_str())
            .map_err(|_| WorkerRequestError::OuterJwsError)?
            .ok_or(WorkerRequestError::OuterJwsError)?;

        debug!("Peeked outer request JWS kid: {}", device_kid);

        // Fetch the corresponding JWK from state using kid
        let device_public_key = state
            .find_device_key(&device_kid)
            .ok_or(WorkerRequestError::OuterJwsError)?
            .public_key
            .clone();

        let outer_request = OuterRequest::from_jws(outer_request_jws.as_str(), &device_public_key)
            .map_err(|_| WorkerRequestError::OuterJwsError)?;

        info!("Received request id {}", request_id);

        // TODO: Use JOSE 'aud' (audience) claim in the validation done inside decode_service_request_jws() instead
        if outer_request.context != "hsm" {
            return Err(WorkerRequestError::UnsupportedContext);
        }

        let session_id = outer_request.session_id.clone();
        let session_key = session_id
            .as_ref()
            .and_then(|id| self.session_key_spi_port.get(id));

        let inner_request = outer_request
            .inner_jwe
            .as_ref()
            .ok_or(WorkerRequestError::InnerJweError)?
            .decrypt_request(
                &self.worker_server_config.server_private_key,
                session_key.as_ref(),
            )
            .map_err(WorkerRequestError::ServiceError)?;

        debug!("Inner request: {:#?}", inner_request);

        let request_type = inner_request.request_type;

        info!(
            "Processing request id {} of type {:?}",
            request_id, request_type
        );

        let operation_context = OperationContext {
            request_id: request_id.clone(),
            state,
            outer_request: outer_request.clone(),
            inner_request,
            session_id: session_id.clone(),
            device_kid: device_kid.clone(),
        };

        let response_context = ResponseContext {
            request_id,
            request_type,
            session_key,
            device_public_key,
        };

        Ok(WorkerInput {
            operation_context,
            response_context,
        })
    }

    fn output(
        &self,
        operation_result: OperationResult,
        context: ResponseContext,
    ) -> Result<WorkerResponse, WorkerRequestError> {
        debug!("Operation result: {:#?}", operation_result.data);

        let encoded_result = operation_result
            .data
            .serialize()
            .map_err(|_| WorkerRequestError::EncryptionError)?;

        let ttl = match operation_result.session_id.as_ref() {
            Some(id) => self.session_key_spi_port.get_remaining_ttl(id),
            None => None,
        };

        // Create InnerResponse with the serialized data
        let serialized_data = String::from_utf8(encoded_result.clone())
            .map_err(|_| WorkerRequestError::EncryptionError)?;
        let inner_response = operation_result.to_inner_response(serialized_data, ttl);

        debug!("Inner response: {:#?}", inner_response);

        let enc_option = context.request_type.encrypt_option();
        debug!(
            "Inner response to {:?} will be encrypted with {:?} encryption",
            context.request_type, enc_option
        );

        // Encrypt the InnerResponse into TypedJwe
        let inner_jwe = match enc_option {
            EncryptOption::Session => {
                let session_key = context
                    .session_key
                    .clone()
                    .ok_or(WorkerRequestError::UnknownSession)?;
                TypedJwe::encrypt(&inner_response, &session_key)
                    .map_err(|_| WorkerRequestError::EncryptionError)?
            }
            EncryptOption::Device => {
                TypedJwe::encrypt_with_jwk(&inner_response, &context.device_public_key)
                    .map_err(|_| WorkerRequestError::EncryptionError)?
            }
        };

        let outer_response = OuterResponse {
            version: 1,
            inner_jwe: Some(inner_jwe),
            session_id: operation_result.session_id.clone(),
        };

        let jws = outer_response
            .to_jws(&self.jws_signer)
            .map_err(|_| WorkerRequestError::OuterJwsError)?;

        let new_state_jws = operation_result
            .state
            .map(|state| state.encode_to_jws(&*self.jws_signer))
            .transpose()
            .map_err(|_| WorkerRequestError::OuterJwsError)?;

        Ok(WorkerResponse {
            request_id: context.request_id,
            http_status: 200,
            state_jws: new_state_jws,
            service_response_jws: jws,
        })
    }
}
