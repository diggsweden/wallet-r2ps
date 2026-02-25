use crate::application::{WorkerResponseError, WorkerResponseSpiPort};
use crate::domain::WorkerResponse;
use crate::infrastructure::KafkaConfig;
use rdkafka::ClientConfig;
use rdkafka::producer::{BaseProducer, BaseRecord};
use std::time::Duration;
use tracing::{debug, error};

pub struct WorkerResponseKafkaSender {
    producer: BaseProducer,
}

impl WorkerResponseKafkaSender {
    pub fn new(config: &KafkaConfig) -> WorkerResponseKafkaSender {
        let producer: BaseProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("broker.address.family", &config.broker_address_family)
            .set("message.timeout.ms", "5000")
            .create()
            .expect("Producer creation failed");

        WorkerResponseKafkaSender { producer }
    }
}

impl WorkerResponseSpiPort for WorkerResponseKafkaSender {
    fn send(&self, worker_response: WorkerResponse) -> Result<(), WorkerResponseError> {
        let response = match serde_json::to_string(&worker_response) {
            Ok(output_json) => {
                let key = &worker_response.request_id;
                let request_id = &worker_response.request_id;
                let record = BaseRecord::to("r2ps-responses")
                    .key(key)
                    .payload(&output_json);

                match self.producer.send(record) {
                    Ok(_) => {
                        // Message enqueued successfully
                        debug!("Message sent: key='{}' request_id='{}'", key, request_id);
                        Ok(())
                    }
                    Err((err, _)) => {
                        error!("Failed to send message: {:?}", err);
                        Err(WorkerResponseError::ConnectionError)
                    }
                }
            }
            Err(e) => {
                error!("Failed to serialize output message: {:?}", e);
                Err(WorkerResponseError::ConnectionError)
            }
        };

        // Poll producer to handle delivery reports and callbacks
        self.producer.poll(Duration::from_millis(100));

        response
    }
}
