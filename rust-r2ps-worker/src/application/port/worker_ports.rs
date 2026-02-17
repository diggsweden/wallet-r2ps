use crate::application::WorkerResponseSpiPort;
use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::pending_auth_spi_port::PendingAuthSpiPort;
use crate::application::session_key_spi_port::SessionKeySpiPort;
use std::sync::Arc;

pub struct WorkerPorts {
    pub worker_response: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    pub session_key: Arc<dyn SessionKeySpiPort + Send + Sync>,
    pub hsm: Arc<dyn HsmSpiPort + Send + Sync>,
    pub pending_auth: Arc<dyn PendingAuthSpiPort + Send + Sync>,
}
