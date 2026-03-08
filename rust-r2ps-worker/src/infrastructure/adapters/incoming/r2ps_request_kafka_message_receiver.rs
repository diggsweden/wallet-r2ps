use crate::application::{WorkerRequestUseCase, WorkerService};
use crate::domain::{HsmWorkerRequest, HsmWorkerRequestDto};
use crate::infrastructure::KafkaConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::{ClientConfig, Message};
use serde_json::from_slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::Duration;
use tracing::{debug, error, warn};

pub struct WorkerRequestKafkaReceiver {
    worker_service: Arc<WorkerService>,
    running: Arc<AtomicBool>,
}

impl WorkerRequestKafkaReceiver {
    pub fn new(
        worker_service: Arc<WorkerService>,
        running: Arc<AtomicBool>,
    ) -> WorkerRequestKafkaReceiver {
        WorkerRequestKafkaReceiver {
            worker_service,
            running,
        }
    }

    pub fn start_worker_thread(&self, config: Arc<KafkaConfig>) -> JoinHandle<()> {
        let worker_service = self.worker_service.clone();
        let running = self.running.clone();

        spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set("group.instance.id", &config.group_instance_id)
                // Cooperative-sticky combines two concepts: sticky assignment
                // (minimizing partition movement) and cooperative
                // rebalancing (incremental, non-blocking rebalances).
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                .set("fetch.wait.max.ms", "500")
                .set("session.timeout.ms", "6000") // Default: 45000ms
                .set("heartbeat.interval.ms", "2000") // Default: 3000ms
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                .set("partition.assignment.strategy", "cooperative-sticky") // Default: 300000ms
                .create()
                .expect("Consumer creation failed");

            // Subscribe to input topic
            consumer
                .subscribe(&["r2ps-requests"])
                .expect("Failed to subscribe to topic");

            debug!("Starting Kafka consumer-producer pipeline...");

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(100)) {
                    Some(Ok(msg)) => {
                        // Extract message payload
                        let payload = match msg.payload() {
                            Some(bytes) => bytes,
                            None => {
                                warn!("Empty message payload");
                                continue;
                            }
                        };

                        let hsm_worker_request_dto: HsmWorkerRequestDto = match from_slice(payload)
                        {
                            Ok(msg) => msg,
                            Err(e) => {
                                error!("Failed to deserialize JSON: {:?}", e);
                                error!("Payload: {:?}", String::from_utf8_lossy(payload));
                                continue;
                            }
                        };

                        // Extract key (optional)
                        let key = msg.key_view::<str>().unwrap();

                        debug!("Received message: key='{:?}'", key);

                        let hsm_worker_request = HsmWorkerRequest {
                            correlation_id: hsm_worker_request_dto.correlation_id,
                            client_id: hsm_worker_request_dto.client_id,
                            request_id: hsm_worker_request_dto.request_id,
                            state_jws: hsm_worker_request_dto.state_jws,
                            outer_request_jws: hsm_worker_request_dto.outer_request_jws,
                        };

                        // Process the message (example: convert to uppercase)
                        match worker_service.execute(hsm_worker_request) {
                            Ok(request_id) => {
                                // Serialize output message to JSON
                                debug!("HsmWorkerRequest {} processed successfully", request_id);
                            }
                            Err(err) => {
                                error!("Error processing message: {:?}", err);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        error!("Kafka error: {}", e);
                    }
                    None => {
                        // No message available, continue polling
                    }
                }
            }
            debug!("Unsubscribing...");
            consumer.unsubscribe();
            drop(consumer);
            debug!("Consumer shutdown complete");
        })
    }
}
