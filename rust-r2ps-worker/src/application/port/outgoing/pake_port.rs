use crate::domain;

#[derive(Debug)]
pub enum PakeError {
    InvalidPasswordFile,
    InvalidRequest,
    AuthStartFailed,
    AuthFinishFailed,
    RegistrationStartFailed,
    UnknownSession,
}

pub struct RegistrationResult {
    pub password_file: domain::PasswordFile,
    pub server_identifier: String,
}

pub trait PakePort: Send + Sync {
    fn registration_start(
        &self,
        request_bytes: &[u8],
        client_id: &str,
    ) -> Result<Vec<u8>, PakeError>;

    fn registration_finish(&self, upload_bytes: &[u8]) -> Result<RegistrationResult, PakeError>;

    fn authentication_start(
        &self,
        request_bytes: &[u8],
        password_file_bytes: &[u8],
        client_id: &str,
        session_id: &domain::SessionId,
    ) -> Result<Vec<u8>, PakeError>;

    fn authentication_finish(
        &self,
        finalization_bytes: &[u8],
        session_id: &domain::SessionId,
        client_id: &str,
    ) -> Result<Vec<u8>, PakeError>;
}
