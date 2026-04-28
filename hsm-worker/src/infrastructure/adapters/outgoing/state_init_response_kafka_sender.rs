// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::state_init_response_spi_port::{
    StateInitResponseError, StateInitResponseSpiPort,
};
use crate::domain::StateInitResponse;
use crate::infrastructure::KafkaConfig;
use rdkafka::ClientConfig;
use rdkafka::producer::{BaseProducer, BaseRecord};
use std::time::Duration;
use tracing::{debug, error};

pub struct StateInitResponseKafkaMessageSender {
    producer: BaseProducer,
}

impl StateInitResponseKafkaMessageSender {
    pub fn new(config: &KafkaConfig) -> Self {
        let producer: BaseProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("broker.address.family", &config.broker_address_family)
            .set("message.timeout.ms", "5000")
            .create()
            .expect("State init response producer creation failed");

        Self { producer }
    }
}

impl StateInitResponseSpiPort for StateInitResponseKafkaMessageSender {
    fn send(
        &self,
        response: StateInitResponse,
        response_topic: &str,
    ) -> Result<(), StateInitResponseError> {
        let output_json = serde_json::to_string(&response).map_err(|e| {
            error!("Failed to serialize state init response: {:?}", e);
            StateInitResponseError::SerializationError
        })?;

        let key = &response.request_id;
        let request_id = &response.request_id;

        let record = BaseRecord::to(response_topic)
            .key(key)
            .payload(&output_json);

        match self.producer.send(record) {
            Ok(_) => {
                debug!(
                    "State init response sent: key='{}' request_id='{}'",
                    key, request_id
                );
                Ok(())
            }
            Err((err, _)) => {
                error!("Failed to send state init response: {:?}", err);
                Err(StateInitResponseError::ConnectionError)
            }
        }?;

        // Poll producer to handle delivery reports
        self.producer.poll(Duration::from_millis(100));

        Ok(())
    }
}
