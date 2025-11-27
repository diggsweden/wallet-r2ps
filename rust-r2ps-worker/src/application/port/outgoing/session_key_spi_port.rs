use std::sync::{Arc, Mutex};
use opaque_ke::{CipherSuite, ClientLoginStartResult, ServerLogin, ServerLoginStartResult};
use crate::domain::{ClientMetadata, DefaultCipherSuite};

pub trait SessionKeySpiPort {
    fn store(&self, pake_session_id: &str, session_key: &SessionKey) -> Result<(), ClientRepositoryError>;
    fn get(&self, pake_session_id: &str) -> Option<SessionKey>;

    fn store_pending_auth(&self, client_id: &str, server_login_start_result: &Arc<LoginSession>);
    fn get_pending_auth(&self, client_id: &str) -> Option<Arc<LoginSession>>;
}

#[derive(Debug)]
pub enum ClientRepositoryError {
    Unknown
}

pub type SessionKey = String;

pub type LoginState = ServerLoginStartResult<DefaultCipherSuite>;

pub struct LoginSession {
    server_login: Mutex<Option<ServerLogin<DefaultCipherSuite>>>,
}

impl LoginSession {
    pub fn new(server_login: ServerLogin<DefaultCipherSuite>) -> Self {
        Self {
            server_login: Mutex::new(Some(server_login)),
        }
    }

    pub fn take(&self) -> Option<ServerLogin<DefaultCipherSuite>> {
        self.server_login.lock().unwrap().take()
    }
}