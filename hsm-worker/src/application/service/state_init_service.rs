// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::StateInitResponseSpiPort;
use crate::application::port::outgoing::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::jose_port::JosePort;
use crate::application::service::TsfHealth;
use crate::domain::{DeviceHsmState, DeviceKeyEntry, StateInitRequest, StateInitResponse};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub struct StateInitService {
    response_spi_port: Arc<dyn StateInitResponseSpiPort + Send + Sync>,
    jose: Arc<dyn JosePort>,
    hsm_spi_port: Arc<dyn HsmSpiPort>,
    hsm_key_label: String,
    opaque_server_id: String,
    health: TsfHealth,
}

#[derive(Debug)]
pub enum StateInitError {
    InvalidJwk,
    InvalidPublicKey(String),
    SigningError,
    SendError,
    HsmKeyGenerationError,
    SelfTestQuarantine,
}

impl StateInitService {
    pub fn new(
        response_spi_port: Arc<dyn StateInitResponseSpiPort + Send + Sync>,
        jose: Arc<dyn JosePort>,
        hsm_spi_port: Arc<dyn HsmSpiPort>,
        hsm_key_label: String,
        opaque_server_id: String,
        health: TsfHealth,
    ) -> Self {
        Self {
            response_spi_port,
            jose,
            hsm_spi_port,
            hsm_key_label,
            opaque_server_id,
            health,
        }
    }

    /// Initialize a new DeviceHsmState for a client
    pub fn initialize(&self, request: StateInitRequest) -> Result<String, StateInitError> {
        debug!("Initializing state, request id: {}", request.request_id);

        if !self.health.is_healthy() {
            error!(
                "Rejecting state-init request {}: TSF self-test suite is unhealthy",
                request.request_id
            );
            return Err(StateInitError::SelfTestQuarantine);
        }

        let response_topic = request.response_topic.clone();

        // 1. Validate client JWK keys (EC P-256)
        validate_ec_public_jwk(&request.client_jws_public_key)?;

        // Resolve JWE key: fall back to JWS key for legacy clients that supply only one key.
        let client_jwe_public_key = match request.client_jwe_public_key {
            Some(k) => {
                validate_ec_public_jwk(&k)?;
                if k.x == request.client_jws_public_key.x && k.y == request.client_jws_public_key.y
                {
                    error!(
                        kid = %request.client_jws_public_key.kid,
                        "Client supplied two keys with identical coordinates — key separation violated"
                    );
                    return Err(StateInitError::InvalidJwk);
                }
                k
            }
            None => {
                warn!(
                    kid = %request.client_jws_public_key.kid,
                    "client_jwe_public_key absent (legacy client); falling back to jws key — key separation not enforced"
                );
                request.client_jws_public_key.clone()
            }
        };

        info!(
            "Initializing state for JWS public key with kid: {} (JWE kid: {})",
            request.client_jws_public_key.kid, client_jwe_public_key.kid
        );

        // 2. Generate dev_authorization_code
        let dev_auth_code = format!("dac_{}", Uuid::new_v4());
        debug!("Generated dev_authorization_code: {}", dev_auth_code);

        // 3. Generate the initial HSM key
        let curve = request.initial_key_curve;

        info!(
            "Generating initial HSM key with curve: {:?} label: {}",
            curve, self.hsm_key_label
        );
        let hsm_key = self
            .hsm_spi_port
            .generate_key(&self.hsm_key_label, &curve)
            .map_err(|e| {
                error!("Failed to generate initial HSM key: {:?}", e);
                StateInitError::HsmKeyGenerationError
            })?;
        let initial_hsm_key = hsm_key.public_key_jwk.clone();
        let hsm_keys = vec![hsm_key];

        // 4. Create DeviceHsmState
        let state = DeviceHsmState {
            version: 1,
            device_keys: vec![DeviceKeyEntry {
                jws_public_key: request.client_jws_public_key,
                jwe_public_key: Some(client_jwe_public_key),
                password_files: vec![],
                dev_authorization_code: Some(dev_auth_code.clone()),
            }],
            hsm_keys,
        };

        debug!("Created initial DeviceHsmState: {:#?}", state);

        // 5. Encode state as JWS
        let state_jws = state.sign(self.jose.as_ref()).map_err(|e| {
            error!("Failed to sign state JWS: {:?}", e);
            StateInitError::SigningError
        })?;

        // 6. Create response
        let response = StateInitResponse {
            request_id: request.request_id.clone(),
            state_jws,
            dev_authorization_code: dev_auth_code,
            server_jws_public_key: self.jose.jws_public_key().clone(),
            server_jwe_public_key: self.jose.jwe_public_key().clone(),
            opaque_server_id: self.opaque_server_id.clone(),
            initial_hsm_key,
        };

        // 6. Send response via Kafka
        self.response_spi_port
            .send(response, &response_topic)
            .map_err(|e| {
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
