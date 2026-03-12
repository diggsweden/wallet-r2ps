use crate::application::port::outgoing::jose_port;
use crate::application::service::operations::OperationResult;
use crate::application::service::worker_service::context::ResponseContext;
use crate::application::service::worker_service::error::{
    ProblemDetail, UpstreamError, WorkerError,
};
use crate::application::session_key_spi_port::SessionKeySpiPort;
use crate::domain::value_objects::r2ps::{InnerResponse, OuterResponse, Status};
use crate::domain::{
    DeviceHsmState, EncryptOption, SessionId, TypedJwe, TypedJws, WorkerRequestError,
    WorkerResponse,
};
use std::sync::Arc;

/// Carries a `WorkerError` with whatever context was available when the error occurred.
pub struct ProcessError {
    pub error: WorkerError,
    pub context: Option<Box<ResponseContext>>,
}

/// Responsible for constructing and signing responses.
/// This includes encrypting inner payloads (JWE) based on the operation's
/// encryption policy (Session vs. Device) and wrapping them in signed outer responses (JWS).
pub struct ResponseBuilder {
    jose: Arc<dyn jose_port::JosePort>,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
}

impl ResponseBuilder {
    pub fn new(
        jose: Arc<dyn jose_port::JosePort>,
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    ) -> Self {
        Self {
            jose,
            session_key_spi_port,
        }
    }

    /// Encodes a successful `OperationResult` into a full `WorkerResponse`.
    pub fn encode_response(
        &self,
        operation_result: OperationResult,
        context: &ResponseContext,
    ) -> Result<WorkerResponse, WorkerError> {
        let inner_response = self.build_inner_response(&operation_result)?;
        let inner_jwe = self.encrypt_inner_response(&inner_response, context)?;
        let outer_response_jws =
            self.build_outer_response_jws(inner_jwe, operation_result.session_id)?;

        let new_state_jws = self.encode_state_jws(operation_result.state)?;

        Ok(WorkerResponse {
            request_id: context.request_id.clone(),
            state_jws: new_state_jws,
            outer_response_jws: Some(outer_response_jws),
            status: Status::Ok,
            error_message: None,
        })
    }

    fn build_inner_response(
        &self,
        operation_result: &OperationResult,
    ) -> Result<InnerResponse, UpstreamError> {
        let encoded_result = operation_result
            .data
            .serialize()
            .map_err(|_| UpstreamError::EncodeFailed("serialize_inner_response_failed"))?;

        let ttl = match operation_result.session_id.as_ref() {
            Some(id) => self.session_key_spi_port.get_remaining_ttl(id),
            None => None,
        };

        let serialized_data = String::from_utf8(encoded_result)
            .map_err(|_| UpstreamError::EncodeFailed("serialize_inner_response_failed"))?;

        Ok(operation_result.to_inner_response(serialized_data, ttl))
    }

    fn build_outer_response_jws(
        &self,
        inner_jwe: TypedJwe<InnerResponse>,
        session_id: Option<SessionId>,
    ) -> Result<TypedJws<OuterResponse>, UpstreamError> {
        OuterResponse::ok(inner_jwe, session_id).sign(self.jose.as_ref())
    }

    fn encode_state_jws(
        &self,
        state: Option<DeviceHsmState>,
    ) -> Result<Option<TypedJws<DeviceHsmState>>, UpstreamError> {
        state
            .map(|state| {
                state
                    .sign(self.jose.as_ref())
                    .map_err(|_| UpstreamError::EncodeFailed("state_sign_failed"))
            })
            .transpose()
    }

    pub fn build_error_response(
        &self,
        request_id: &str,
        process_err: ProcessError,
    ) -> Result<WorkerResponse, WorkerRequestError> {
        let ProcessError { error, context } = process_err;
        let problem_json = error.to_problem_details_json(request_id);

        match error {
            WorkerError::Upstream(_) => {
                Ok(self.build_upstream_only_error_response(request_id, problem_json))
            }
            WorkerError::Outer(_) => {
                self.build_outer_error_worker_response(request_id, problem_json)
            }
            WorkerError::Inner(_) => {
                let context = context
                    .as_ref()
                    .ok_or(WorkerRequestError::ResponseBuildError)?;
                match self.encrypt_inner_response(&InnerResponse::error(problem_json), context) {
                    Ok(inner_jwe) => self.sign_and_wrap_outer_response(
                        request_id,
                        OuterResponse::ok(inner_jwe, None),
                    ),
                    Err(_) => Err(WorkerRequestError::ResponseBuildError),
                }
            }
        }
    }

    fn build_outer_error_worker_response(
        &self,
        request_id: &str,
        problem_json: String,
    ) -> Result<WorkerResponse, WorkerRequestError> {
        self.sign_and_wrap_outer_response(request_id, OuterResponse::error(problem_json))
    }

    pub fn build_upstream_only_error_response(
        &self,
        request_id: &str,
        problem_json: String,
    ) -> WorkerResponse {
        WorkerResponse {
            request_id: request_id.to_string(),
            state_jws: None,
            outer_response_jws: None,
            status: Status::Error,
            error_message: Some(problem_json),
        }
    }

    fn encrypt_inner_response(
        &self,
        inner_response: &InnerResponse,
        context: &ResponseContext,
    ) -> Result<TypedJwe<InnerResponse>, UpstreamError> {
        let enc_option = context.request_type.encrypt_option();
        let enc_key = match enc_option {
            EncryptOption::Session => {
                let session_key = context
                    .session_key
                    .as_ref()
                    .ok_or(UpstreamError::EncodeFailed("unknown_session"))?;
                jose_port::JweEncryptionKey::Session(session_key)
            }
            EncryptOption::Device => {
                jose_port::JweEncryptionKey::Device(&context.device_public_key)
            }
        };

        inner_response.encrypt(self.jose.as_ref(), enc_key)
    }

    fn sign_and_wrap_outer_response(
        &self,
        request_id: &str,
        outer_response: OuterResponse,
    ) -> Result<WorkerResponse, WorkerRequestError> {
        match outer_response.sign(self.jose.as_ref()) {
            Ok(outer_response_jws) => Ok(WorkerResponse {
                request_id: request_id.to_string(),
                state_jws: None,
                outer_response_jws: Some(outer_response_jws),
                status: Status::Ok,
                error_message: None,
            }),
            Err(_) => Err(WorkerRequestError::ResponseBuildError),
        }
    }
}
