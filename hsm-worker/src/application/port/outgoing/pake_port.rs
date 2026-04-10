// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::session_state_spi_port::{PendingLoginState, SessionKey};
use crate::domain;
use crate::domain::PakePayloadVector;
use crate::domain::value_objects::client_metadata::PasswordFileEntry;

#[derive(Debug)]
pub enum PakeError {
    InvalidPasswordFile,
    InvalidRequest,
    AuthStartFailed,
    AuthFinishFailed,
    RegistrationStartFailed,
}

pub struct RegistrationResult {
    pub password_file: domain::PasswordFile,
    /// Domain separator of the OPAQUE server keypair used at registration time
    pub opaque_domain_separator: String,
}

#[cfg_attr(test, mockall::automock)]
pub trait PakePort: Send + Sync {
    fn registration_start(
        &self,
        request_bytes: &[u8],
        client_id: &str,
    ) -> Result<PakePayloadVector, PakeError>;

    fn registration_finish(&self, upload_bytes: &[u8]) -> Result<RegistrationResult, PakeError>;

    fn authentication_start(
        &self,
        request_bytes: &[u8],
        password_file_entry: &PasswordFileEntry,
        client_id: &str,
    ) -> Result<(PakePayloadVector, PendingLoginState), PakeError>;

    fn authentication_finish(
        &self,
        finalization_bytes: &[u8],
        pending_state: &PendingLoginState,
        client_id: &str,
    ) -> Result<SessionKey, PakeError>;
}
