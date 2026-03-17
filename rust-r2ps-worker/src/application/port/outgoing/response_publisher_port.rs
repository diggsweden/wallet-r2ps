/// Port for publishing responses directly to Kafka (bypassing the outbox).
/// Used for read-only operations and error responses where atomic guarantees are not needed.
pub trait ResponsePublisher: Send + Sync {
    fn publish_response(&self, device_id: &str, payload: &[u8]) -> Result<(), String>;
}
