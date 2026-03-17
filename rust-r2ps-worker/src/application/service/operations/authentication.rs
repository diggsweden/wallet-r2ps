use super::{OperationContext, OperationResult, ServiceOperation, SessionTransition};
use crate::application::port::outgoing::pake_port;
use crate::application::port::outgoing::session_state_spi_port::{OngoingOperation, SessionState};
use crate::domain;
use std::sync::Arc;
use tracing::warn;

fn pake_err_to_service_err(e: pake_port::PakeError) -> domain::ServiceRequestError {
    match e {
        pake_port::PakeError::InvalidPasswordFile => {
            domain::ServiceRequestError::InvalidSerializedPasswordFile
        }
        pake_port::PakeError::InvalidRequest => {
            domain::ServiceRequestError::InvalidAuthenticateRequest
        }
        pake_port::PakeError::AuthStartFailed => {
            domain::ServiceRequestError::ServerLoginStartFailed
        }
        pake_port::PakeError::AuthFinishFailed => {
            domain::ServiceRequestError::ServerLoginFinishFailed
        }
        pake_port::PakeError::RegistrationStartFailed => {
            domain::ServiceRequestError::ServerRegistrationStartFailed
        }
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
    fn execute(
        &self,
        context: OperationContext,
    ) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_request = domain::PakeRequest::from_inner_request(context.inner_request)?;

        let password_file = context
            .state
            .get_password_file(&context.device_kid)
            .ok_or(domain::ServiceRequestError::UnknownClient)?;

        let session_id = domain::SessionId::new();

        let (response, pending_state) = self
            .pake_port
            .authentication_start(
                pake_request.data.as_ref(),
                password_file.as_bytes(),
                &context.device_kid,
            )
            .map_err(pake_err_to_service_err)?;

        let payload = domain::PakeResponse {
            data: Some(response),
        };

        Ok(OperationResult {
            state: None,
            data: domain::InnerResponseData::new(payload)?,
            session_id: Some(session_id),
            session_transition: Some(SessionTransition::CreatePendingAuth {
                pending_state,
                purpose: pake_request.purpose,
            }),
        })
    }
}

// AuthenticateFinish Operation
pub struct AuthenticateFinishOperation {
    pake_port: Arc<dyn pake_port::PakePort>,
}

impl AuthenticateFinishOperation {
    pub fn new(pake_port: Arc<dyn pake_port::PakePort>) -> Self {
        Self { pake_port }
    }
}

impl ServiceOperation for AuthenticateFinishOperation {
    fn execute(
        &self,
        context: OperationContext,
    ) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_request = domain::PakeRequest::from_inner_request(context.inner_request)?;

        let session_id = context
            .session_id
            .as_ref()
            .ok_or(domain::ServiceRequestError::UnknownSession)?;

        let pending = match &context.session_state {
            Some(SessionState::PendingAuth(data)) => data,
            _ => return Err(domain::ServiceRequestError::UnknownSession),
        };

        let session_key = self
            .pake_port
            .authentication_finish(
                pake_request.data.as_ref(),
                &pending.server_login,
                &context.device_kid,
            )
            .map_err(|e| {
                warn!("authentication finish failed: {:?}", e);
                pake_err_to_service_err(e)
            })?;

        let payload = domain::PakeResponse { data: None };

        Ok(OperationResult {
            state: None,
            data: domain::InnerResponseData::new(payload)?,
            session_id: Some(session_id.clone()),
            session_transition: Some(SessionTransition::Authenticate { session_key }),
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
    fn execute(
        &self,
        context: OperationContext,
    ) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_request = domain::PakeRequest::from_inner_request(context.inner_request)?;

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
            return Err(domain::ServiceRequestError::InvalidAuthorizationCode);
        }

        let response = self
            .pake_port
            .registration_start(pake_request.data.as_ref(), &context.device_kid)
            .map_err(pake_err_to_service_err)?;

        let payload = domain::PakeResponse {
            data: Some(response),
        };

        Ok(OperationResult {
            state: None,
            data: domain::InnerResponseData::new(payload)?,
            session_id: context.session_id,
            session_transition: None,
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
    fn execute(
        &self,
        context: OperationContext,
    ) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_payload = domain::PakeRequest::from_inner_request(context.inner_request)?;

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
            return Err(domain::ServiceRequestError::InvalidAuthorizationCode);
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
        new_state.set_password_file(
            &context.device_kid,
            password_file_entry,
            pake_payload.authorization.as_deref(),
        )?;

        let payload = domain::PakeResponse { data: None };

        Ok(OperationResult {
            state: Some(new_state),
            data: domain::InnerResponseData::new(payload)?,
            session_id: context.session_id,
            session_transition: None,
        })
    }
}

// PinChangeStart Operation
// Initiates PIN change by starting a new OPAQUE registration within an authenticated session.
// Replaces the existing password file with a new one (the password file is stored as a list
// to support future enhancements).
// Future improvement: Support parallel authentication where both PINs remain valid until
// the next login. This would require the server to return challenges for both password files
// during AuthenticateStart (since the server can't determine which PIN the client has), then
// verify against all candidates during AuthenticateFinish. The system would self-heal by
// discarding unused credentials after successful verification, preventing user lockout during
// network failures.
pub struct PinChangeStartOperation {
    pake_port: Arc<dyn pake_port::PakePort>,
}

impl PinChangeStartOperation {
    pub fn new(pake_port: Arc<dyn pake_port::PakePort>) -> Self {
        Self { pake_port }
    }
}

impl ServiceOperation for PinChangeStartOperation {
    fn execute(
        &self,
        context: OperationContext,
    ) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_request = domain::PakeRequest::from_inner_request(context.inner_request)?;

        if context.session_id.is_none() {
            return Err(domain::ServiceRequestError::UnknownSession);
        }

        let response = self
            .pake_port
            .registration_start(pake_request.data.as_ref(), &context.device_kid)
            .map_err(pake_err_to_service_err)?;

        let payload = domain::PakeResponse {
            data: Some(response),
        };

        Ok(OperationResult {
            state: None,
            data: domain::InnerResponseData::new(payload)?,
            session_id: context.session_id,
            session_transition: Some(SessionTransition::BeginChangingPin),
        })
    }
}

// PinChangeFinish Operation
pub struct PinChangeFinishOperation {
    pake_port: Arc<dyn pake_port::PakePort>,
}

impl PinChangeFinishOperation {
    pub fn new(pake_port: Arc<dyn pake_port::PakePort>) -> Self {
        Self { pake_port }
    }
}

impl ServiceOperation for PinChangeFinishOperation {
    fn execute(
        &self,
        context: OperationContext,
    ) -> Result<OperationResult, domain::ServiceRequestError> {
        let pake_payload = domain::PakeRequest::from_inner_request(context.inner_request)?;

        let is_changing_pin = matches!(
            &context.session_state,
            Some(SessionState::Active(data)) if matches!(data.operation, Some(OngoingOperation::ChangingPin))
        );
        if !is_changing_pin {
            return Err(domain::ServiceRequestError::InvalidOperation);
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
        new_state.set_password_file(&context.device_kid, password_file_entry, None)?;

        let payload = domain::PakeResponse { data: None };

        Ok(OperationResult {
            state: Some(new_state),
            data: domain::InnerResponseData::new(payload)?,
            session_id: context.session_id,
            session_transition: Some(SessionTransition::End),
        })
    }
}
