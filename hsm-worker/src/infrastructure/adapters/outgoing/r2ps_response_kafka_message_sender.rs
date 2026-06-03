// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::{WorkerResponseError, WorkerResponseSpiPort};
use crate::domain::HsmWorkerResponse;
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
            // acks=1 (leader-only): a response lost on broker crash is acceptable
            // because the BFF correlation map will time out and surface failure to
            // the caller. Idempotence is not used here — request_id is the dedupe key
            // applied at the application layer.
            .set("acks", "1")
            .set("linger.ms", "5")
            .set("batch.size", "65536")
            .set("compression.type", "lz4")
            .create()
            .expect("Producer creation failed");

        WorkerResponseKafkaSender { producer }
    }
}

impl WorkerResponseSpiPort for WorkerResponseKafkaSender {
    fn send(
        &self,
        worker_response: HsmWorkerResponse,
        response_topic: &str,
    ) -> Result<(), WorkerResponseError> {
        let response = match serde_json::to_string(&worker_response) {
            Ok(output_json) => {
                let key = &worker_response.request_id;
                let request_id = &worker_response.request_id;
                let record = BaseRecord::to(response_topic)
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

        // Drain any ready delivery callbacks without blocking. The 100ms used
        // here previously was the dominant per-request latency floor — librdkafka's
        // background thread does the actual produce; this call only services the
        // (unused) callback queue.
        self.producer.poll(Duration::from_millis(0));

        response
    }
}
