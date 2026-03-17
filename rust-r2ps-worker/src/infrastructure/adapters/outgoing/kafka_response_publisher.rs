use crate::application::port::outgoing::response_publisher_port::ResponsePublisher;
use rdkafka::producer::{BaseProducer, BaseRecord};
use rdkafka::ClientConfig;
use std::time::Duration;
use tracing::{debug, error};

/// Direct Kafka publisher for responses (bypasses outbox).
/// Used for read-only operations and error responses.
/// Low-latency config: acks=1, linger.ms=0.
pub struct KafkaResponsePublisher {
    producer: BaseProducer,
}

impl KafkaResponsePublisher {
    pub fn new(bootstrap_servers: &str) -> Self {
        let producer: BaseProducer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("acks", "1")
            .set("linger.ms", "0")
            .set("socket.nagle.disable", "true")
            .create()
            .expect("Failed to create Kafka response publisher");

        Self { producer }
    }
}

impl ResponsePublisher for KafkaResponsePublisher {
    fn publish_response(&self, device_id: &str, payload: &[u8]) -> Result<(), String> {
        self.producer
            .send(
                BaseRecord::to("r2ps-responses")
                    .key(device_id)
                    .payload(payload),
            )
            .map_err(|(e, _)| {
                error!("Failed to publish response: {:?}", e);
                format!("Kafka send error: {:?}", e)
            })?;

        // Trigger immediate delivery from librdkafka's internal buffer
        self.producer.poll(Duration::from_millis(0));

        debug!("Published response directly for device_id={}", device_id);
        Ok(())
    }
}
