use std::sync::Arc;

use crate::adapters::outbound::{
    kafka::KafkaMessagePublisher,
    redis::{RedisClientKeyRepository, RedisRequestRepository},
};
use crate::application::request_processing::SubmitRequestUseCase;

use super::auth::HpkeAuthContext;
use super::registry::ClientConnectionRegistry;

/// Concrete application state for WebSocket handlers.
///
/// Holds the HPKE auth context, client registry, and use cases needed
/// for WebSocket request processing.
pub struct WsAppState {
    /// HPKE authentication context (server key pair).
    pub hpke_auth: HpkeAuthContext,

    /// In-memory registry of connected clients on this pod.
    pub registry: ClientConnectionRegistry,

    /// Use case for submitting service requests (shared with HTTP adapter).
    pub submit_request_use_case: SubmitRequestUseCase<
        RedisClientKeyRepository,
        RedisRequestRepository,
        KafkaMessagePublisher,
    >,

    /// Client key repository for looking up device public keys during auth.
    /// Populated by the state-snapshot Kafka consumer.
    pub key_repo: RedisClientKeyRepository,
}

/// Shared WebSocket state, wrapped in `Arc` for use across handlers.
pub type SharedWsState = Arc<WsAppState>;
