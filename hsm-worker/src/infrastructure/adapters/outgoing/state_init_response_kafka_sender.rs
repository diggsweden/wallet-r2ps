// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::state_init_response_spi_port::{
    StateInitResponseError, StateInitResponseSpiPort,
};
use crate::domain::StateInitResponse;
use crate::infrastructure::KafkaConfig;
use rdkafka::ClientConfig;
use rdkafka::producer::{BaseRecord, ThreadedProducer};
use tracing::{debug, error};

pub struct StateInitResponseKafkaMessageSender {
    producer: ThreadedProducer<rdkafka::producer::DefaultProducerContext>,
}

impl StateInitResponseKafkaMessageSender {
    pub fn new(config: &KafkaConfig) -> Self {
        // ThreadedProducer drains delivery callbacks on its own thread,
        // removing the per-send poll(0) the BaseProducer required. See
        // WorkerResponseKafkaSender for further rationale.
        let producer: ThreadedProducer<_> = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("broker.address.family", &config.broker_address_family)
            .set("message.timeout.ms", "5000")
            // acks=1 (leader-only): see WorkerResponseKafkaSender for rationale.
            .set("acks", "1")
            // See WorkerResponseKafkaSender for rationale.
            .set("linger.ms", config.producer_linger_ms.to_string())
            .set("batch.size", "1048576")
            .set("compression.type", "lz4")
            .set("queue.buffering.max.messages", "1000000")
            .set("queue.buffering.max.kbytes", "524288")
            // See request_sender.rs in wallet-bff for rationale.
            .set("socket.nagle.disable", "true")
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
        let output_json = serde_json::to_vec(&response).map_err(|e| {
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
        }
    }
}
