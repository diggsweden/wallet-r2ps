use crate::application::StateInitResponseSpiPort;
use crate::domain::{DeviceHsmState, DeviceKeyEntry, StateInitRequest, StateInitResponse};
use josekit::jwk::Jwk;
use josekit::jws::alg::ecdsa::EcdsaJwsSigner;
use std::sync::Arc;
use tracing::{debug, error, info};
use uuid::Uuid;

pub struct StateInitService {
    response_spi_port: Arc<dyn StateInitResponseSpiPort + Send + Sync>,
    jws_signer: Arc<EcdsaJwsSigner>,
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
        jws_signer: Arc<EcdsaJwsSigner>,
    ) -> Self {
        Self {
            response_spi_port,
            jws_signer,
        }
    }

    /// Initialize a new DeviceHsmState for a client
    pub fn initialize(&self, request: StateInitRequest) -> Result<String, StateInitError> {
        debug!("Initializing state, request id: {}", request.request_id);

        // 1. Convert EcPublicJwk to josekit Jwk
        let device_key = ec_public_jwk_to_jwk(&request.public_key)?;

        // 2. Validate public_key JWK (EC P-256)
        validate_ec_public_jwk(&request.public_key)?;

        info!(
            "Initializing state for public key with kid: {}",
            request.public_key.kid
        );

        // 3. Generate dev_authorization_code
        let dev_auth_code = format!("dac_{}", Uuid::new_v4());
        debug!("Generated dev_authorization_code: {}", dev_auth_code);

        // 4. Create DeviceHsmState
        let state = DeviceHsmState {
            version: 1,
            device_keys: vec![DeviceKeyEntry {
                public_key: device_key,
                password_files: vec![],
                dev_authorization_code: Some(dev_auth_code.clone()),
            }],
            hsm_keys: vec![],
        };

        debug!("Created initial DeviceHsmState: {:#?}", state);

        // 5. Encode state as JWS
        let state_jws = state.encode_to_jws(&*self.jws_signer).map_err(|e| {
            error!("Failed to encode state JWS: {:?}", e);
            StateInitError::SigningError
        })?;

        // 6. Create response
        let response = StateInitResponse {
            request_id: request.request_id.clone(),
            state_jws,
            dev_authorization_code: dev_auth_code,
        };

        // 7. Send response via Kafka
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

/// Converts EcPublicJwk to josekit Jwk
fn ec_public_jwk_to_jwk(ec_jwk: &crate::domain::EcPublicJwk) -> Result<Jwk, StateInitError> {
    let mut jwk = Jwk::new("EC");
    jwk.set_curve(&ec_jwk.crv);
    jwk.set_parameter("x", Some(serde_json::Value::String(ec_jwk.x.clone())))
        .map_err(|_| StateInitError::InvalidJwk)?;
    jwk.set_parameter("y", Some(serde_json::Value::String(ec_jwk.y.clone())))
        .map_err(|_| StateInitError::InvalidJwk)?;
    jwk.set_key_id(&ec_jwk.kid);

    Ok(jwk)
}
