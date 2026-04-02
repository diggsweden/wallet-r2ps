use crate::application::WorkerResponseSpiPort;
use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::jose_port::JosePort;
use crate::application::port::outgoing::pake_port::PakePort;
use crate::application::port::outgoing::session_state_spi_port::SessionStateSpiPort;
use std::sync::Arc;

pub struct WorkerPorts {
    pub jose: Arc<dyn JosePort>,
    pub worker_response: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    pub session_state: Arc<dyn SessionStateSpiPort>,
    pub hsm: Arc<dyn HsmSpiPort + Send + Sync>,
    pub pake: Arc<dyn PakePort>,
}
