use super::{OperationContext, OperationResult, ServiceOperation};
use crate::application::hsm_spi_port::HsmSpiPort;
use crate::domain::{
    CreateKeyServiceData, CreateKeyServiceDataResponse, DeleteKeyServiceData, DeviceHsmState,
    InnerResponseData, KeyInfo, ListKeysResponse, ServiceRequestError, SignRequest,
};
use std::sync::Arc;
use tracing::debug;

pub struct HsmEcdsaSignOperation {
    hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
}

impl HsmEcdsaSignOperation {
    pub fn new(hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>) -> Self {
        Self { hsm_spi_port }
    }
}

impl ServiceOperation for HsmEcdsaSignOperation {
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
            .find(|key| key.public_key_jwk.kid.eq(&sign_request.kid))
            .cloned()
            .ok_or(ServiceRequestError::UnknownKey)?;

        let raw_sig_bytes = self
            .hsm_spi_port
            .sign(&hsm_key, &sign_request.tbs_hash)
            .map_err(|_| ServiceRequestError::Unknown)?;

        let signature = p256::ecdsa::Signature::from_slice(&raw_sig_bytes)
            .map_err(|_| ServiceRequestError::Unknown)?;
        let asn1_signature: Vec<u8> = signature.to_der().as_bytes().to_vec();

        debug!("Hsm Ecdsa asn1_signature: {:?}", asn1_signature);

        Ok(OperationResult {
            state: context.state,
            data: InnerResponseData::Asn1Signature(asn1_signature),
            session_id: context.session_id,
        })
    }
}

pub struct HsmKeygenOperation {
    hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
}

impl HsmKeygenOperation {
    pub fn new(hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>) -> Self {
        Self { hsm_spi_port }
    }
}

impl ServiceOperation for HsmKeygenOperation {
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
            client_id: context.state.client_id,
            wallet_id: context.state.wallet_id,
            client_public_key: context.state.client_public_key,
            password_file: context.state.password_file,
            keys: new_keys,
        };

        Ok(OperationResult {
            state: new_state,
            data: InnerResponseData::CreateKey(CreateKeyServiceDataResponse {
                public_key: hsm_key.public_key_jwk,
            }),
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
            client_id: context.state.client_id,
            wallet_id: context.state.wallet_id,
            client_public_key: context.state.client_public_key,
            password_file: context.state.password_file,
            keys: context
                .state
                .keys
                .into_iter()
                .filter(|key| key.public_key_jwk.kid != payload.kid)
                .collect(),
        };

        Ok(OperationResult {
            state: new_state,
            data: InnerResponseData::DeleteKey(DeleteKeyServiceData { kid: payload.kid }),
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
            data: InnerResponseData::ListKeys(payload),
            session_id: context.session_id,
        })
    }
}
