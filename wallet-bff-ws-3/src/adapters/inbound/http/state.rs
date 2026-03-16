use crate::adapters::outbound::{
    kafka::KafkaMessagePublisher,
    redis::{RedisClientKeyRepository, RedisRequestRepository, RedisStateInitRepository},
};
use crate::application::{
    device_management::InitializeDeviceUseCase,
    request_processing::{PollRequestUseCase, SubmitRequestUseCase},
};

/// Concrete application state passed to HTTP handlers.
///
/// Holds the use cases wired with their concrete adapter implementations.
/// This is the only place in the codebase that knows about concrete types.
pub struct HttpAppState {
    pub submit_request_use_case: SubmitRequestUseCase<
        RedisClientKeyRepository,
        RedisRequestRepository,
        KafkaMessagePublisher,
    >,
    pub poll_request_use_case: PollRequestUseCase<RedisRequestRepository>,
    pub init_device_use_case: InitializeDeviceUseCase<
        RedisClientKeyRepository,
        KafkaMessagePublisher,
        RedisStateInitRepository,
    >,
    pub serve_sync: bool,
    pub base_url: String,
}
