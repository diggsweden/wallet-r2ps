use crate::application::port::outgoing::jose_port;
use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};

use crate::application::{
    WorkerPorts, WorkerRequestId, WorkerRequestUseCase, WorkerResponseSpiPort,
};
use crate::domain::value_objects::r2ps::OuterRequest;
use crate::domain::{
    DeviceHsmState, EcPublicJwk, EncryptOption, HsmWorkerRequest, OperationId, OuterResponse,
    WorkerRequestError, WorkerResponse,
};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};

use super::operations::{OperationContext, OperationDispatcher, OperationResult};

pub struct WorkerService {
    jose: Arc<dyn jose_port::JosePort>,
    worker_response_spi_port: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    operation_dispatcher: OperationDispatcher,
}

impl WorkerService {
    pub fn new(jose: Arc<dyn jose_port::JosePort>, ports: WorkerPorts) -> Self {
        let operation_dispatcher = OperationDispatcher::from_dependencies(
            ports.pake,
            ports.session_key.clone(), // TODO: Make OperationDispatcher side-effect free - remove this
            ports.hsm,
        );

        Self {
            jose,
            worker_response_spi_port: ports.worker_response,
            operation_dispatcher,
            session_key_spi_port: ports.session_key,
        }
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
    device_public_key: EcPublicJwk,
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

        let state = DeviceHsmState::from_jws(state_jws.as_str(), self.jose.as_ref())
            .map_err(|_| WorkerRequestError::InvalidState)?;

        // Extract client public key kid from the JWS header
        let device_kid = self
            .jose
            .peek_kid(outer_request_jws.as_str())
            .map_err(|_| WorkerRequestError::OuterJwsError)?
            .ok_or(WorkerRequestError::OuterJwsError)?;

        debug!("Peeked outer request JWS kid: {}", device_kid);

        // Fetch the corresponding JWK from state using kid
        let device_public_key = state
            .find_device_key(&device_kid)
            .ok_or(WorkerRequestError::OuterJwsError)?
            .public_key
            .clone();

        let outer_request =
            OuterRequest::from_jws(outer_request_jws.as_str(), self.jose.as_ref(), &device_public_key)?;

        info!("Received request id {}", request_id);

        // TODO: Use JOSE 'aud' (audience) claim in the validation done inside decode_service_request_jws() instead
        if outer_request.context != "hsm" {
            return Err(WorkerRequestError::UnsupportedContext);
        }

        let session_id = outer_request.session_id.clone();
        let session_key = session_id
            .as_ref()
            .and_then(|id| self.session_key_spi_port.get(id));

        let inner_request = outer_request.decrypt_inner(self.jose.as_ref(), session_key.as_ref())?;

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
        let serialized_data =
            String::from_utf8(encoded_result).map_err(|_| WorkerRequestError::EncryptionError)?;
        let inner_response = operation_result.to_inner_response(serialized_data, ttl);

        debug!("Inner response: {:#?}", inner_response);

        let enc_option = context.request_type.encrypt_option();
        debug!(
            "Inner response to {:?} will be encrypted with {:?} encryption",
            context.request_type, enc_option
        );

        // Encrypt the InnerResponse into TypedJwe
        let enc_key = match enc_option {
            EncryptOption::Session => {
                let session_key = context
                    .session_key
                    .as_ref()
                    .ok_or(WorkerRequestError::UnknownSession)?;
                jose_port::JweEncryptionKey::Session(session_key)
            }
            EncryptOption::Device => {
                jose_port::JweEncryptionKey::Device(&context.device_public_key)
            }
        };
        let inner_jwe = inner_response.encrypt(self.jose.as_ref(), enc_key)?;

        let outer_response = OuterResponse {
            version: 1,
            inner_jwe: Some(inner_jwe),
            session_id: operation_result.session_id.clone(),
        };

        let jws = outer_response.sign(self.jose.as_ref())?;

        let new_state_jws = operation_result
            .state
            .map(|state| {
                state
                    .sign(self.jose.as_ref())
                    .map_err(|_| WorkerRequestError::OuterJwsError)
            })
            .transpose()?;

        Ok(WorkerResponse {
            request_id: context.request_id,
            http_status: 200,
            state_jws: new_state_jws,
            service_response_jws: jws,
        })
    }
}
