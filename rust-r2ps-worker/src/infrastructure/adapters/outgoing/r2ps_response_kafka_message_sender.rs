use crate::application::{R2psResponseError, R2psResponseSpiPort};
use crate::domain::R2PsResponse;
use crate::infrastructure::KafkaConfig;
use rdkafka::ClientConfig;
use rdkafka::producer::{BaseProducer, BaseRecord};
use std::time::Duration;
use tracing::{error, info};

pub struct R2psResponseKafkaMessageSender {
    producer: BaseProducer,
}

impl R2psResponseKafkaMessageSender {
    pub fn new(config: &KafkaConfig) -> R2psResponseKafkaMessageSender {
        let producer: BaseProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("broker.address.family", &config.broker_address_family)
            .set("message.timeout.ms", "5000")
            .create()
            .expect("Producer creation failed");

        R2psResponseKafkaMessageSender { producer }
    }
}

impl R2psResponseSpiPort for R2psResponseKafkaMessageSender {
    fn send(&self, r2ps_response: R2PsResponse) -> Result<(), R2psResponseError> {
        let response = match serde_json::to_string(&r2ps_response) {
            Ok(output_json) => {
                let key = r2ps_response.wallet_id;
                let request_id = r2ps_response.request_id.clone();
                let record = BaseRecord::to("r2ps-responses")
                    .key(&key)
                    .payload(&output_json);

                match self.producer.send(record) {
                    Ok(_) => {
                        // Message enqueued successfully
                        info!("Message sent: key='{}' request_id='{}'", key, request_id);
                        Ok(())
                    }
                    Err((err, _)) => {
                        error!("Failed to send message: {:?}", err);
                        Err(R2psResponseError::ConnectionError)
                    }
                }
            }
            Err(e) => {
                error!("Failed to serialize output message: {:?}", e);
                Err(R2psResponseError::ConnectionError)
            }
        };

        // Poll producer to handle delivery reports and callbacks
        self.producer.poll(Duration::from_millis(100));

        response
    }
}
