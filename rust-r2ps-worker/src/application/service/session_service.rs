use crate::application::pending_auth_spi_port::PendingAuthSpiPort;
use crate::application::session_key_spi_port::SessionKeySpiPort;
use std::sync::Arc;

#[derive(Clone)]
pub struct SessionService {
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
}

impl SessionService {
    pub fn new(
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
        pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
    ) -> SessionService {
        Self {
            session_key_spi_port,
            pending_auth_spi_port,
        }
    }
}
