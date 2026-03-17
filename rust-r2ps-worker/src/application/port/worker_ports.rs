use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::pake_port::PakePort;
use crate::application::port::outgoing::response_publisher_port::ResponsePublisher;
use crate::application::port::outgoing::state_cache_port::{StateCache, TamperDetectionCache};
use crate::application::port::outgoing::state_repository_port::StateRepository;
use crate::application::session_key_spi_port::SessionKeySpiPort;
use std::sync::Arc;

pub struct WorkerPorts {
    pub state_repository: Arc<dyn StateRepository>,
    pub response_publisher: Arc<dyn ResponsePublisher>,
    pub tamper_cache: Arc<dyn TamperDetectionCache>,
    pub state_cache: Arc<dyn StateCache>,
    pub session_key: Arc<dyn SessionKeySpiPort + Send + Sync>,
    pub hsm: Arc<dyn HsmSpiPort + Send + Sync>,
    pub pake: Arc<dyn PakePort>,
}
