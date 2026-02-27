use crate::application::port::outgoing::jose_port;
use crate::domain::{DeviceHsmState, TypedJws};
use tracing::error;

#[derive(Debug)]
pub enum DeviceStateError {
    SerializeError,
    SignError,
    VerifyError,
}

impl DeviceHsmState {
    pub fn sign(
        &self,
        jose: &dyn jose_port::JosePort,
    ) -> Result<TypedJws<DeviceHsmState>, DeviceStateError> {
        let bytes = serde_json::to_vec(self).map_err(|e| {
            error!("Failed to serialize state: {:?}", e);
            DeviceStateError::SerializeError
        })?;
        let jws_str = jose.jws_sign(&bytes).map_err(|e| {
            error!("Failed to sign state JWS: {:?}", e);
            DeviceStateError::SignError
        })?;
        Ok(TypedJws::new(jws_str))
    }

    pub fn from_jws(jws: &str, jose: &dyn jose_port::JosePort) -> Result<Self, DeviceStateError> {
        let bytes = jose.jws_verify_server(jws).map_err(|e| {
            error!("State JWS verification failed: {:?}", e);
            DeviceStateError::VerifyError
        })?;
        serde_json::from_slice(&bytes).map_err(|e| {
            error!("Failed to deserialize state: {:?}", e);
            DeviceStateError::VerifyError
        })
    }
}
