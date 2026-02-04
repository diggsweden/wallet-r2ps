use super::{OperationContext, OperationResult, ServiceOperation};
use crate::application::hsm_spi_port::HsmSpiPort;
use crate::define_byte_vector;
use crate::domain::{
    CreateKeyServiceData, CreateKeyServiceDataResponse, DeleteKeyServiceData, DeviceHsmState,
    InnerResponseData, KeyInfo, ListKeysResponse, ServiceRequestError, SignRequest,
    SignatureResponse,
};
use std::sync::Arc;
use tracing::debug;

pub struct HsmSignOperation {
    hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
}

impl HsmSignOperation {
    pub fn new(hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>) -> Self {
        Self { hsm_spi_port }
    }
}

define_byte_vector!(SignatureVector);
define_byte_vector!(MessageVector);

impl ServiceOperation for HsmSignOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let data = context
            .inner_request
            .data
            .ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let sign_request = serde_json::from_slice::<SignRequest>(data.as_bytes())
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;

        let hsm_key = context
            .state
            .keys
            .iter()
            .find(|key| key.public_key_jwk.kid.eq(&sign_request.hsm_kid))
            .cloned()
            .ok_or(ServiceRequestError::UnknownKey)?;

        let raw_sig_bytes = self
            .hsm_spi_port
            .sign(&hsm_key, &sign_request.message)
            .map_err(|_| ServiceRequestError::Unknown)?;

        let signature = p256::ecdsa::Signature::from_slice(&raw_sig_bytes)
            .map_err(|_| ServiceRequestError::Unknown)?;
        let signature = SignatureVector::new(signature.to_der().as_bytes().to_vec());

        debug!("HSM ECDSA ASN.1 signature: {:?}", signature);

        Ok(OperationResult {
            state: context.state,
            data: InnerResponseData::new(SignatureResponse { signature })?,
            session_id: context.session_id,
        })
    }
}

pub struct HsmGenerateKeyOperation {
    hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
}

impl HsmGenerateKeyOperation {
    pub fn new(hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>) -> Self {
        Self { hsm_spi_port }
    }
}

impl ServiceOperation for HsmGenerateKeyOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let data = context
            .inner_request
            .data
            .ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let payload = serde_json::from_slice::<CreateKeyServiceData>(data.as_bytes())
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;

        let hsm_key = self
            .hsm_spi_port
            .generate_key("foobar", &payload.curve)
            .map_err(|_| ServiceRequestError::Unknown)?;

        let mut new_keys = context.state.keys.clone();
        new_keys.push(hsm_key.clone());

        let new_state = DeviceHsmState {
            keys: new_keys,
            ..context.state
        };

        Ok(OperationResult {
            state: new_state,
            data: InnerResponseData::new(CreateKeyServiceDataResponse {
                public_key: hsm_key.public_key_jwk,
            })?,
            session_id: context.session_id,
        })
    }
}

pub struct HsmDeleteKeyOperation;

impl ServiceOperation for HsmDeleteKeyOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let data = context
            .inner_request
            .data
            .ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let payload = serde_json::from_slice::<DeleteKeyServiceData>(data.as_bytes())
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;

        let new_state = DeviceHsmState {
            keys: context
                .state
                .keys
                .into_iter()
                .filter(|key| key.public_key_jwk.kid != payload.hsm_kid)
                .collect(),
            ..context.state
        };

        Ok(OperationResult {
            state: new_state,
            data: InnerResponseData::new(DeleteKeyServiceData {
                hsm_kid: payload.hsm_kid,
            })?,
            session_id: context.session_id,
        })
    }
}

pub struct HsmListKeysOperation;

impl ServiceOperation for HsmListKeysOperation {
    fn execute(&self, context: OperationContext) -> Result<OperationResult, ServiceRequestError> {
        let payload = ListKeysResponse {
            key_info: context
                .state
                .keys
                .iter()
                .map(|key| KeyInfo {
                    public_key: key.public_key_jwk.clone(),
                    created_at: Some(key.created_at.to_rfc3339()),
                })
                .collect(),
        };

        Ok(OperationResult {
            state: context.state,
            data: InnerResponseData::new(payload)?,
            session_id: context.session_id,
        })
    }
}
