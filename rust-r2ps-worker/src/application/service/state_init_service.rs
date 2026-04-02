use crate::application::StateInitResponseSpiPort;
use crate::application::port::outgoing::jose_port::JosePort;
use crate::domain::{DeviceHsmState, DeviceKeyEntry, StateInitRequest, StateInitResponse};
use std::sync::Arc;
use tracing::{debug, error, info};
use uuid::Uuid;

pub struct StateInitService {
    response_spi_port: Arc<dyn StateInitResponseSpiPort + Send + Sync>,
    jose: Arc<dyn JosePort>,
    opaque_server_id: String,
}

#[derive(Debug)]
pub enum StateInitError {
    InvalidJwk,
    InvalidPublicKey(String),
    SigningError,
    SendError,
}

impl StateInitService {
    pub fn new(
        response_spi_port: Arc<dyn StateInitResponseSpiPort + Send + Sync>,
        jose: Arc<dyn JosePort>,
        opaque_server_id: String,
    ) -> Self {
        Self {
            response_spi_port,
            jose,
            opaque_server_id,
        }
    }

    /// Initialize a new DeviceHsmState for a client
    pub fn initialize(&self, request: StateInitRequest) -> Result<String, StateInitError> {
        debug!("Initializing state, request id: {}", request.request_id);

        // 1. Validate public_key JWK (EC P-256)
        validate_ec_public_jwk(&request.public_key)?;

        info!(
            "Initializing state for public key with kid: {}",
            request.public_key.kid
        );

        // 2. Generate dev_authorization_code
        let dev_auth_code = format!("dac_{}", Uuid::new_v4());
        debug!("Generated dev_authorization_code: {}", dev_auth_code);

        // 3. Create DeviceHsmState
        let state = DeviceHsmState {
            version: 1,
            device_keys: vec![DeviceKeyEntry {
                public_key: request.public_key,
                password_files: vec![],
                dev_authorization_code: Some(dev_auth_code.clone()),
            }],
            hsm_keys: vec![],
        };

        debug!("Created initial DeviceHsmState: {:#?}", state);

        // 4. Encode state as JWS
        let state_jws = state.sign(self.jose.as_ref()).map_err(|e| {
            error!("Failed to sign state JWS: {:?}", e);
            StateInitError::SigningError
        })?;

        // 5. Create response
        let response = StateInitResponse {
            request_id: request.request_id.clone(),
            state_jws,
            dev_authorization_code: dev_auth_code,
            server_jws_public_key: self.jose.jws_public_key().clone(),
            server_jws_kid: self.jose.jws_kid().to_owned(),
            opaque_server_id: self.opaque_server_id.clone(),
        };

        // 6. Send response via Kafka
        self.response_spi_port.send(response).map_err(|e| {
            error!("Failed to send state init response: {:?}", e);
            StateInitError::SendError
        })?;

        info!(
            "State initialization complete for request_id: {}",
            request.request_id
        );
        Ok(request.request_id)
    }
}

/// Validates EcPublicJwk is EC P-256
fn validate_ec_public_jwk(jwk: &crate::domain::EcPublicJwk) -> Result<(), StateInitError> {
    if jwk.kty != "EC" {
        error!("Invalid JWK: key type must be EC, got: {}", jwk.kty);
        return Err(StateInitError::InvalidJwk);
    }

    if jwk.crv != "P-256" {
        error!("Invalid JWK: curve must be P-256, got: {}", jwk.crv);
        return Err(StateInitError::InvalidJwk);
    }

    if jwk.x.is_empty() || jwk.y.is_empty() {
        error!("Invalid JWK: missing x or y coordinate");
        return Err(StateInitError::InvalidJwk);
    }

    if jwk.kid.is_empty() {
        error!("Invalid JWK: missing kid");
        return Err(StateInitError::InvalidJwk);
    }

    Ok(())
}
