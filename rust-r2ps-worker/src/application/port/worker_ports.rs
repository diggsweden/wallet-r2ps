use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::port::outgoing::pake_port::PakePort;
use crate::application::port::outgoing::response_publisher_port::ResponsePublisher;
use crate::application::port::outgoing::session_state_spi_port::SessionStateSpiPort;
use crate::application::port::outgoing::state_cache_port::{StateCache, TamperDetectionCache};
use crate::application::port::outgoing::state_repository_port::StateRepository;
use std::sync::Arc;

pub struct WorkerPorts {
    /// Persistent HSM state repository (PostgreSQL-backed)
    pub state_repository: Arc<dyn StateRepository>,
    /// Kafka response publisher (for read-only/error responses)
    pub response_publisher: Arc<dyn ResponsePublisher>,
    /// Tamper detection cache (redb monotonic high-water mark)
    pub tamper_cache: Arc<dyn TamperDetectionCache>,
    /// In-memory state cache (Moka)
    pub state_cache: Arc<dyn StateCache>,
    /// Session state FSM (ephemeral OPAQUE session lifecycle)
    pub session_state: Arc<dyn SessionStateSpiPort>,
    pub hsm: Arc<dyn HsmSpiPort + Send + Sync>,
    pub pake: Arc<dyn PakePort>,
}
