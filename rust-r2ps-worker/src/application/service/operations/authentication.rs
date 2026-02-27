use super::{OperationContext, OperationResult, ServiceOperation};
use crate::application::port::outgoing::pake_port;
use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use crate::domain;
use std::sync::Arc;
use tracing::warn;

fn pake_err_to_service_err(e: pake_port::PakeError) -> domain::ServiceRequestError {
    match e {
        pake_port::PakeError::InvalidPasswordFile => domain::ServiceRequestError::InvalidSerializedPasswordFile,
        pake_port::PakeError::InvalidRequest => domain::ServiceRequestError::InvalidAuthenticateRequest,
        pake_port::PakeError::AuthStartFailed => domain::ServiceRequestError::ServerLoginStartFailed,
        pake_port::PakeError::AuthFinishFailed => domain::ServiceRequestError::ServerLoginFinishFailed,
        pake_port::PakeError::RegistrationStartFailed => domain::ServiceRequestError::ServerRegistrationStartFailed,
        pake_port::PakeError::UnknownSession => domain::ServiceRequestError::UnknownSession,
    }
}

// AuthenticateStart Operation
pub struct AuthenticateStartOperation {
    pake_port: Arc<dyn pake_port::PakePort>,
}

impl AuthenticateStartOperation {
    pub fn new(pake_port: Arc<dyn pake_port::PakePort>) -> Self {
        Self { pake_port }
    }
}

impl ServiceOperation for AuthenticateStartOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_request = domain::PakeRequest::from_inner_request(context.inner_request)?;

        let password_file = context
            .state
            .get_password_file(&context.device_kid)
            .ok_or(domain::ServiceRequestError::UnknownClient)?;

        let session_id = domain::SessionId::new();

        let response_bytes = self
            .pake_port
            .authentication_start(
                pake_request.data.as_ref(),
                password_file.as_bytes(),
                &context.device_kid,
                &session_id,
            )
            .map_err(pake_err_to_service_err)?;

        let payload = domain::PakeResponse {
            task: None,
            data: Some(domain::PakePayloadVector::new(response_bytes)),
        };

        Ok(OperationResult {
            state: None,
            data: domain::InnerResponseData::new(payload)?,
            session_id: Some(session_id),
        })
    }
}

// AuthenticateFinish Operation
pub struct AuthenticateFinishOperation {
    pake_port: Arc<dyn pake_port::PakePort>,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
}

impl AuthenticateFinishOperation {
    pub fn new(
        pake_port: Arc<dyn pake_port::PakePort>,
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    ) -> Self {
        Self {
            pake_port,
            session_key_spi_port,
        }
    }
}

impl ServiceOperation for AuthenticateFinishOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_request = domain::PakeRequest::from_inner_request(context.inner_request)?;

        let session_id = context
            .session_id
            .as_ref()
            .ok_or(domain::ServiceRequestError::UnknownSession)?;

        let session_key_bytes = self
            .pake_port
            .authentication_finish(
                pake_request.data.as_ref(),
                session_id,
                &context.device_kid,
            )
            .map_err(|e| {
                warn!("authentication finish failed: {:?}", e);
                pake_err_to_service_err(e)
            })?;

        let session_key = SessionKey::new(session_key_bytes);

        self.session_key_spi_port
            .store(session_id, session_key)
            .map_err(|_| domain::ServiceRequestError::InternalServerError)?;

        let payload = domain::PakeResponse {
            task: None,
            data: None,
        };

        Ok(OperationResult {
            state: None,
            data: domain::InnerResponseData::new(payload)?,
            session_id: Some(session_id.clone()),
        })
    }
}

// RegisterStart Operation
pub struct RegisterStartOperation {
    pake_port: Arc<dyn pake_port::PakePort>,
}

impl RegisterStartOperation {
    pub fn new(pake_port: Arc<dyn pake_port::PakePort>) -> Self {
        Self { pake_port }
    }
}

impl ServiceOperation for RegisterStartOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_request = domain::PakeRequest::from_inner_request(context.inner_request)?;

        // TODO: require authorization code (currently optional)
        if let Some(provided_code) = &pake_request.authorization {
            let device_key = context
                .state
                .find_device_key(&context.device_kid)
                .ok_or(domain::ServiceRequestError::UnknownKey)?;
            if device_key.dev_authorization_code.as_deref() != Some(provided_code.as_str()) {
                warn!("authorization code mismatch in register start");
                return Err(domain::ServiceRequestError::InvalidAuthorizationCode);
            }
        } else {
            warn!("missing authorization code in register start");
            // TODO: Enable this
            // return Err(domain::ServiceRequestError::InvalidAuthorizationCode);
        }

        let response_bytes = self
            .pake_port
            .registration_start(pake_request.data.as_ref(), &context.device_kid)
            .map_err(pake_err_to_service_err)?;

        let payload = domain::PakeResponse {
            task: None,
            data: Some(domain::PakePayloadVector::new(response_bytes)),
        };

        Ok(OperationResult {
            state: None,
            data: domain::InnerResponseData::new(payload)?,
            session_id: context.session_id,
        })
    }
}

// RegisterFinish Operation
pub struct RegisterFinishOperation {
    pake_port: Arc<dyn pake_port::PakePort>,
}

impl RegisterFinishOperation {
    pub fn new(pake_port: Arc<dyn pake_port::PakePort>) -> Self {
        Self { pake_port }
    }
}

impl ServiceOperation for RegisterFinishOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_payload = domain::PakeRequest::from_inner_request(context.inner_request)?;

        // TODO: require authorization code (currently optional)
        if let Some(provided_code) = &pake_payload.authorization {
            let device_key = context
                .state
                .find_device_key(&context.device_kid)
                .ok_or(domain::ServiceRequestError::UnknownKey)?;
            if device_key.dev_authorization_code.as_deref() != Some(provided_code.as_str()) {
                warn!("authorization code mismatch in register finish");
                return Err(domain::ServiceRequestError::InvalidAuthorizationCode);
            }
        } else {
            warn!("missing authorization code in register finish");
            // TODO: Enable this
            // return Err(domain::ServiceRequestError::InvalidAuthorizationCode);
        }

        let pake_port::RegistrationResult {
            password_file,
            server_identifier,
        } = self
            .pake_port
            .registration_finish(pake_payload.data.as_ref())
            .map_err(pake_err_to_service_err)?;

        let password_file_entry = domain::PasswordFileEntry {
            password_file,
            server_identifier,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let mut new_state = context.state;
        new_state.add_password_file(
            &context.device_kid,
            password_file_entry,
            pake_payload.authorization.as_deref(),
        )?;

        let payload = domain::PakeResponse {
            task: None,
            data: None,
        };

        Ok(OperationResult {
            state: Some(new_state),
            data: domain::InnerResponseData::new(payload)?,
            session_id: context.session_id,
        })
    }
}
