// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::{WorkerResponseError, WorkerResponseSpiPort};
use crate::domain::HsmWorkerResponse;
use crate::infrastructure::KafkaConfig;
use rdkafka::ClientConfig;
use rdkafka::message::{Header, OwnedHeaders};
use rdkafka::producer::{BaseRecord, ThreadedProducer};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error};

fn now_epoch_us_bytes() -> Vec<u8> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
        .to_string()
        .into_bytes()
}

pub struct WorkerResponseKafkaSender {
    producer: ThreadedProducer<rdkafka::producer::DefaultProducerContext>,
}

impl WorkerResponseKafkaSender {
    pub fn new(config: &KafkaConfig) -> WorkerResponseKafkaSender {
        // ThreadedProducer spawns a dedicated polling thread that drains
        // delivery-report callbacks for us, replacing the per-send poll(0)
        // we previously called from every worker-partition thread. Same
        // .send() API as BaseProducer.
        let producer: ThreadedProducer<_> = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("broker.address.family", &config.broker_address_family)
            .set("message.timeout.ms", "5000")
            // acks=1 (leader-only): a response lost on broker crash is acceptable
            // because the BFF correlation map will time out and surface failure to
            // the caller. Idempotence is not used here — request_id is the dedupe key
            // applied at the application layer.
            .set("acks", "1")
            // linger.ms=0: hsm-worker response traffic fans out across two BFF
            // response topics and many partitions; per-partition rate is low
            // enough that any non-zero linger.ms becomes a per-response latency
            // floor with negligible batching gain. batch.size still bounds bursts.
            .set("linger.ms", "0")
            .set("batch.size", "65536")
            .set("compression.type", "lz4")
            // See request_sender.rs in wallet-bff for rationale.
            .set("socket.nagle.disable", "true")
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
        match serde_json::to_vec(&worker_response) {
            Ok(output_json) => {
                let key = &worker_response.request_id;
                let request_id = &worker_response.request_id;
                let t_buf = now_epoch_us_bytes();
                let headers = OwnedHeaders::new().insert(Header {
                    key: "t_produced_us",
                    value: Some(t_buf.as_slice()),
                });
                let record = BaseRecord::to(response_topic)
                    .key(key)
                    .payload(&output_json)
                    .headers(headers);

                let t = Instant::now();
                let send_result = self.producer.send(record);
                let send_us = t.elapsed().as_micros();
                match send_result {
                    Ok(_) => {
                        // Message enqueued in librdkafka's internal queue. The
                        // actual broker shipping happens asynchronously on the
                        // librdkafka background thread; send_us only covers the
                        // user-side enqueue.
                        debug!(
                            request_id = %request_id,
                            response_topic,
                            send_us,
                            "response producer enqueue"
                        );
                        Ok(())
                    }
                    Err((err, _)) => {
                        error!(
                            request_id = %request_id,
                            response_topic,
                            send_us,
                            "Failed to send message: {:?}",
                            err
                        );
                        Err(WorkerResponseError::ConnectionError)
                    }
                }
            }
            Err(e) => {
                error!("Failed to serialize output message: {:?}", e);
                Err(WorkerResponseError::ConnectionError)
            }
        }
    }
}
