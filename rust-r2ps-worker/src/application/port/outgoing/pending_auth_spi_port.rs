use crate::domain::{DefaultCipherSuite, SessionId};
use opaque_ke::{ServerLogin, ServerLoginStartResult};
use std::sync::{Arc, Mutex};

pub trait PendingAuthSpiPort {
    fn store_pending_auth(&self, id: &SessionId, server_login_start_result: &Arc<LoginSession>);
    fn get_pending_auth(&self, id: &SessionId) -> Option<Arc<LoginSession>>;
}

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
