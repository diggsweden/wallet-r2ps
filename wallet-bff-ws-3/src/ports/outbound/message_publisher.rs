use crate::domain::hsm_integration::{
    entities::{StateInitRequest, WorkerRequest},
    errors::HsmError,
};

/// Port for publishing messages to the HSM worker.
///
/// Abstracts the messaging infrastructure (Kafka, in-memory, etc.)
/// so the application layer is decoupled from the message bus.
///
/// Both regular service requests and state-init commands are published
/// to the same `r2ps-requests` topic — the worker distinguishes them
/// by the `context` field in the outer request.
pub trait MessagePublisher: Send + Sync {
    /// Send a service request to the HSM worker for processing.
    fn publish_worker_request(
        &self,
        request: &WorkerRequest,
    ) -> impl Future<Output = Result<(), HsmError>> + Send;

    /// Send a state initialization request to the HSM worker.
    ///
    /// Published to the same `r2ps-requests` topic with `context: "state-init"`.
    fn publish_state_init_request(
        &self,
        request: &StateInitRequest,
    ) -> impl Future<Output = Result<(), HsmError>> + Send;
}
