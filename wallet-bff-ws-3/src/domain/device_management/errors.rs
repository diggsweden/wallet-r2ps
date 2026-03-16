use super::value_objects::{ClientIdError, DeviceStateError, EcPublicKeyError};

/// Domain errors for the Device Management bounded context.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("device not found: {0}")]
    NotFound(String),

    #[error("device already exists: {0}")]
    AlreadyExists(String),

    #[error("invalid client ID: {0}")]
    InvalidClientId(#[from] ClientIdError),

    #[error("invalid device state: {0}")]
    InvalidDeviceState(#[from] DeviceStateError),

    #[error("invalid public key: {0}")]
    InvalidPublicKey(#[from] EcPublicKeyError),

    #[error("storage error: {0}")]
    StorageError(String),
}
