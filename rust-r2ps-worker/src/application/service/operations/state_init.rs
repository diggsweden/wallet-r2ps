use crate::application::service::operations::{
    OperationContext, OperationResult, ServiceOperation,
};
use crate::domain::{
    DeviceHsmState, DeviceKeyEntry, InnerResponseData, ServiceRequestError, StateInitInnerResponse,
};
use tracing::{debug, error};
use uuid::Uuid;

/// StateInit operation — creates a version 0 DeviceHsmState with the device's public key.
pub struct StateInitOperation;

impl ServiceOperation for StateInitOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        // Extract the public key from the inner request data
        let inner_data = context
            .inner_request
            .data
            .as_ref()
            .ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;

        let init_request: crate::domain::StateInitInnerRequest = serde_json::from_str(inner_data)
            .map_err(|e| {
            error!("Failed to deserialize StateInitInnerRequest: {:?}", e);
            ServiceRequestError::InvalidServiceRequestFormat
        })?;

        // Validate EC P-256 public key
        validate_ec_public_jwk(&init_request.public_key)?;

        // Generate one-time authorization code
        let dev_auth_code = format!("dac_{}", Uuid::new_v4());
        debug!("Generated dev_authorization_code: {}", dev_auth_code);

        // Create genesis state (version 0)
        let state = DeviceHsmState {
            version: 0,
            device_keys: vec![DeviceKeyEntry {
                public_key: init_request.public_key,
                password_files: vec![],
                dev_authorization_code: Some(dev_auth_code.clone()),
            }],
            hsm_keys: vec![],
        };

        let response_data = StateInitInnerResponse {
            dev_authorization_code: dev_auth_code,
            device_id: context.device_id.clone(),
        };

        Ok(OperationResult {
            state: Some(state),
            data: InnerResponseData::new(response_data)?,
            session_id: None,
        })
    }
}

/// Validates EcPublicJwk is EC P-256
fn validate_ec_public_jwk(jwk: &crate::domain::EcPublicJwk) -> Result<(), ServiceRequestError> {
    if jwk.kty != "EC" {
        error!("Invalid JWK: key type must be EC, got: {}", jwk.kty);
        return Err(ServiceRequestError::InvalidPublicKey);
    }

    if jwk.crv != "P-256" {
        error!("Invalid JWK: curve must be P-256, got: {}", jwk.crv);
        return Err(ServiceRequestError::InvalidPublicKey);
    }

    if jwk.x.is_empty() || jwk.y.is_empty() {
        error!("Invalid JWK: missing x or y coordinate");
        return Err(ServiceRequestError::InvalidPublicKey);
    }

    if jwk.kid.is_empty() {
        error!("Invalid JWK: missing kid");
        return Err(ServiceRequestError::InvalidPublicKey);
    }

    Ok(())
}
