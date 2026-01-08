use crate::application::permit_list_use_case::PermitListDto;
use crate::application::service::device_metadata_service::DeviceMetadataService;
use crate::infrastructure::KafkaConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::{ClientConfig, Message};
use serde_json::from_slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::Duration;
use tracing::{debug, error, info, warn};

pub struct PermitListKafkaMessageReceiver {
    device_metadata_service: Arc<DeviceMetadataService>,
    running: Arc<AtomicBool>,
}

impl PermitListKafkaMessageReceiver {
    pub fn new(
        device_metadata_service: Arc<DeviceMetadataService>,
        running: Arc<AtomicBool>,
    ) -> PermitListKafkaMessageReceiver {
        Self {
            device_metadata_service,
            running,
        }
    }

    pub fn start_worker_thread(&self, config: KafkaConfig) -> JoinHandle<()> {
        let bootstrap_servers = String::from(config.bootstrap_servers);
        let broker_address_family = String::from(config.broker_address_family);
        let group_id = String::from(config.group_id);
        let group_instance_id = String::from(config.group_instance_id); // todo

        let device_metadata_service = self.device_metadata_service.clone();
        let running = self.running.clone();

        spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &bootstrap_servers)
                .set("broker.address.family", &broker_address_family)
                .set("group.id", "&group_id-something-else")
                .set("group.instance.id", &group_instance_id)
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
                .subscribe(&["wallet-permit-list"])
                .expect("Failed to subscribe to topic");

            info!("Starting Kafka consumer wallet-permit-list...");

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

                        let input_msg: PermitListDto = match from_slice(payload) {
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

                        // Process the message (example: convert to uppercase)
                        match device_metadata_service
                            .update_device_permit_list(input_msg.device_id, input_msg)
                        {
                            Ok(_) => {
                                // Serialize output message to JSON
                                info!("device permit list item received");
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
            info!("Unsubscribing...");
            consumer.unsubscribe();
            drop(consumer);
            info!("Consumer shutdown complete");
        })
    }
}
