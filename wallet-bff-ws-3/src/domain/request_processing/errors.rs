use super::value_objects::SignedJwsError;

/// Domain errors for the Request Processing bounded context.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("request not found: {0}")]
    NotFound(String),

    #[error("invalid JWS: {0}")]
    InvalidJws(#[from] SignedJwsError),

    #[error("timeout waiting for HSM response")]
    Timeout,

    #[error("storage error: {0}")]
    StorageError(String),

    #[error("messaging error: {0}")]
    MessagingError(String),
}
