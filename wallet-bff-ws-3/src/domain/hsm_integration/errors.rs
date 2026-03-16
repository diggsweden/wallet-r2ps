/// Domain errors for the HSM Integration bounded context.
#[derive(Debug, thiserror::Error)]
pub enum HsmError {
    #[error("failed to send message to HSM worker: {0}")]
    SendFailed(String),

    #[error("timeout waiting for HSM worker response")]
    Timeout,

    #[error("invalid response from HSM worker: {0}")]
    InvalidResponse(String),

    #[error("storage error: {0}")]
    StorageError(String),
}
