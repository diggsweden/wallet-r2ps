use super::{OperationContext, OperationResult, ServiceOperation};
use crate::application::pending_auth_spi_port::{LoginSession, PendingAuthSpiPort};
use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use crate::domain::value_objects::r2ps::{PakeRequest, PakeResponse};
use crate::domain::{
    DefaultCipherSuite, DeviceHsmState, InnerResponseData, PakePayloadVector, PasswordFile,
    ServiceRequestError, SessionId,
};
use argon2::password_hash::rand_core::OsRng;
use opaque_ke::{
    CredentialFinalization, CredentialRequest, Identifiers, RegistrationRequest,
    RegistrationUpload, ServerLogin, ServerLoginParameters, ServerRegistration, ServerSetup,
};
use std::sync::Arc;
use tracing::{debug, warn};

/// Creates OPAQUE ServerLoginParameters with standardized context and identifiers
fn create_server_login_parameters<'a: 'b, 'b>(device_id: &'a str) -> ServerLoginParameters<'a, 'b> {
    let context = b"RPS-Ops";
    let client = device_id.as_bytes();
    let server = b"https://cloud-wallet.digg.se/rhsm";

    debug!(
        "OPAQUE context: '{}' hex: {}",
        String::from_utf8_lossy(context),
        hex::encode(context)
    );
    debug!(
        "OPAQUE client: '{}' hex: {}",
        String::from_utf8_lossy(client),
        hex::encode(client)
    );
    debug!(
        "OPAQUE server: '{}' hex: {}",
        String::from_utf8_lossy(server),
        hex::encode(server)
    );

    ServerLoginParameters {
        context: Some(context),
        identifiers: Identifiers {
            client: Some(client),
            server: Some(server),
        },
    }
}

// AuthenticateStart Operation
pub struct AuthenticateStartOperation {
    opaque_server_setup: ServerSetup<DefaultCipherSuite>,
    pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
}

impl AuthenticateStartOperation {
    pub fn new(
        opaque_server_setup: ServerSetup<DefaultCipherSuite>,
        pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
    ) -> Self {
        Self {
            opaque_server_setup,
            pending_auth_spi_port,
        }
    }
}

impl ServiceOperation for AuthenticateStartOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let pake_request = PakeRequest::from_inner_request(context.inner_request)?;

        debug!(
            "deserialized pake payload authenticate request data: {:?}",
            pake_request.data
        );

        let password_file_serialized = context
            .state
            .password_file
            .as_ref()
            .ok_or(ServiceRequestError::UnknownClient)?;
        let password_file = ServerRegistration::<DefaultCipherSuite>::deserialize(
            password_file_serialized.as_bytes(),
        )
        .map_err(|e| {
            warn!("error decoding pake request: {:?}", e);
            ServiceRequestError::InvalidSerializedPasswordFile
        })?;

        let decoded_request_data = pake_request.data.as_ref();
        let credential_request =
            CredentialRequest::deserialize(&decoded_request_data).map_err(|e| {
                warn!("error decoding pake request: {:?}", e);
                ServiceRequestError::InvalidAuthenticateRequest
            })?;

        let mut server_rng = OsRng;
        let server_login_parameters = create_server_login_parameters(&context.device_id);

        let server_login_start_result = ServerLogin::start(
            &mut server_rng,
            &self.opaque_server_setup,
            Some(password_file),
            credential_request,
            context.device_id.as_bytes(),
            server_login_parameters,
        )
        .map_err(|e| {
            warn!("error decoding pake request: {:?}", e);
            ServiceRequestError::ServerLoginStartFailed
        })?;

        let payload_bytes = server_login_start_result.message.serialize();
        let payload = PakePayloadVector::new(payload_bytes.to_vec());

        let session_id = SessionId::new();

        self.pending_auth_spi_port.store_pending_auth(
            &session_id,
            &Arc::new(LoginSession::new(server_login_start_result.state)),
        );

        let payload = PakeResponse {
            task: None,
            data: Some(payload),
        };

        Ok(OperationResult {
            state: context.state,
            data: InnerResponseData::new(payload)?,
            session_id: Some(session_id),
        })
    }
}

// AuthenticateFinish Operation
pub struct AuthenticateFinishOperation {
    pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
}

impl AuthenticateFinishOperation {
    pub fn new(
        pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    ) -> Self {
        Self {
            pending_auth_spi_port,
            session_key_spi_port,
        }
    }
}

impl ServiceOperation for AuthenticateFinishOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let pake_request = PakeRequest::from_inner_request(context.inner_request)?;

        let decoded_request_data = pake_request.data.as_ref();

        // Get the pending auth session id which was created in the start phase and sent to the client,
        // which the client now returns back to us to finish the authentication.
        let session_id = context
            .session_id
            .as_ref()
            .ok_or(ServiceRequestError::UnknownSession)?;

        let session = self
            .pending_auth_spi_port
            .get_pending_auth(&session_id)
            .ok_or(ServiceRequestError::UnknownSession)?;

        let server_login_parameters = create_server_login_parameters(&context.device_id);

        let server_login = session
            .take()
            .ok_or(ServiceRequestError::InvalidAuthenticateRequest)?;
        let result = server_login
            .finish(
                CredentialFinalization::deserialize(&decoded_request_data)
                    .map_err(|_| ServiceRequestError::InvalidAuthenticateRequest)?,
                server_login_parameters,
            )
            .map_err(|e| {
                warn!("could not finish auth request request: {:?}", e);
                ServiceRequestError::ServerLoginFinishFailed
            })?;

        let session_key = SessionKey::new(result.session_key.to_vec());
        debug!("Derived shared session key: {:?}", session_key);

        self.session_key_spi_port
            .store(&session_id, session_key)
            .map_err(|_| ServiceRequestError::InternalServerError)?;

        let payload = PakeResponse {
            task: None,
            data: None,
        };

        Ok(OperationResult {
            state: context.state,
            data: InnerResponseData::new(payload)?,
            session_id: Some(session_id.clone()),
        })
    }
}

// RegisterStart Operation
pub struct RegisterStartOperation {
    opaque_server_setup: ServerSetup<DefaultCipherSuite>,
}

impl RegisterStartOperation {
    pub fn new(opaque_server_setup: ServerSetup<DefaultCipherSuite>) -> Self {
        Self {
            opaque_server_setup,
        }
    }
}

impl ServiceOperation for RegisterStartOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let pake_request = PakeRequest::from_inner_request(context.inner_request)?;

        debug!("deserialized pake request {:?}", pake_request.data);

        let decoded_request_data = pake_request.data.as_ref();

        let registration_request = RegistrationRequest::deserialize(&decoded_request_data)
            .map_err(|e| {
                warn!("invalid registration request evaluate: {:?}", e);
                ServiceRequestError::InvalidRegistrationRequest
            })?;

        let server_registration_start_result = ServerRegistration::<DefaultCipherSuite>::start(
            &self.opaque_server_setup,
            registration_request,
            context.device_id.as_bytes(),
        )
        .map_err(|e| {
            warn!("invalid registration request evaluate: {:?}", e);
            ServiceRequestError::ServerRegistrationStartFailed
        })?;

        debug!(
            "server_registration_start_result: {:?}",
            server_registration_start_result.message
        );

        let payload_bytes = server_registration_start_result.message.serialize();
        let payload = PakePayloadVector::new(payload_bytes.to_vec());

        let payload = PakeResponse {
            task: None,
            data: Some(payload),
        };

        Ok(OperationResult {
            state: context.state,
            data: InnerResponseData::new(payload)?,
            session_id: context.session_id,
        })
    }
}

// RegisterFinish Operation
pub struct RegisterFinishOperation;

impl RegisterFinishOperation {
    pub fn new() -> Self {
        Self
    }
}

impl ServiceOperation for RegisterFinishOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let inner_request = context.inner_request;
        let pake_payload = PakeRequest::from_inner_request(inner_request)?;

        let decoded_request_data = pake_payload.data.as_ref();

        let registration_request: RegistrationUpload<DefaultCipherSuite> =
            RegistrationUpload::deserialize(&decoded_request_data).map_err(|e| {
                warn!("invalid registration request finalize: {:?}", e);
                ServiceRequestError::InvalidRegistrationRequest
            })?;

        let password_file = ServerRegistration::<DefaultCipherSuite>::finish(registration_request);
        let password_file_serialized = password_file.serialize();
        debug!(
            "password file: {:?}",
            hex::encode(&password_file_serialized)
        );

        let new_state = DeviceHsmState {
            password_file: Some(PasswordFile(password_file_serialized)),
            keys: Vec::new(),
            ..context.state
        };

        let payload = PakeResponse {
            task: None,
            data: None,
        };

        Ok(OperationResult {
            state: new_state,
            data: InnerResponseData::new(payload)?,
            session_id: context.session_id,
        })
    }
}
