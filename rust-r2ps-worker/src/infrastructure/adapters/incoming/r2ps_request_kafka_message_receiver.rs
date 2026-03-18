use crate::application::WorkerRequestUseCase;
use crate::domain::{HsmWorkerRequest, HsmWorkerRequestDto, StateInitCommandDto};
use crate::infrastructure::KafkaConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::{ClientConfig, Message};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{JoinHandle, spawn};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Discriminated command type from the r2ps-requests topic.
enum IncomingCommand {
    WorkerRequest(HsmWorkerRequest),
    StateInit(StateInitCommandDto),
}

pub struct WorkerRequestKafkaReceiver {
    worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync>,
    running: Arc<AtomicBool>,
}

impl WorkerRequestKafkaReceiver {
    pub fn new(
        worker_use_case: Arc<dyn WorkerRequestUseCase + Send + Sync>,
        running: Arc<AtomicBool>,
    ) -> WorkerRequestKafkaReceiver {
        WorkerRequestKafkaReceiver {
            worker_use_case,
            running,
        }
    }

    pub fn start_worker_thread(&self, config: Arc<KafkaConfig>) -> JoinHandle<()> {
        let worker_use_case = self.worker_use_case.clone();
        let running = self.running.clone();

        spawn(move || {
            let consumer: BaseConsumer = ClientConfig::new()
                .set("bootstrap.servers", &config.bootstrap_servers)
                .set("broker.address.family", &config.broker_address_family)
                .set("group.id", &config.group_id)
                .set("group.instance.id", &config.group_instance_id)
                .set("partition.assignment.strategy", "cooperative-sticky")
                .set("enable.auto.commit", "true")
                .set("auto.offset.reset", "earliest")
                .set("fetch.wait.max.ms", "10")
                .set("session.timeout.ms", "6000")
                .set("heartbeat.interval.ms", "2000")
                .set("max.poll.interval.ms", "300000")
                .set("connections.max.idle.ms", "540000")
                .set("metadata.max.age.ms", "5000")
                .create()
                .expect("Consumer creation failed");

            consumer
                .subscribe(&["r2ps-requests"])
                .expect("Failed to subscribe to topic");

            info!("Command consumer started on r2ps-requests topic");

            while running.load(Ordering::Relaxed) {
                match consumer.poll(Duration::from_millis(10)) {
                    Some(Ok(msg)) => {
                        let payload = match msg.payload() {
                            Some(bytes) => bytes,
                            None => {
                                warn!("Empty message payload");
                                continue;
                            }
                        };

                        let key = msg.key_view::<str>().unwrap();
                        debug!("Received message: key='{:?}'", key);

                        match deserialize_command(payload) {
                            Some(IncomingCommand::WorkerRequest(req)) => {
                                let t0 = std::time::Instant::now();
                                match worker_use_case.execute(req) {
                                    Ok(id) => {
                                        info!("Command {} processed in {:?}", id, t0.elapsed());
                                    }
                                    Err(err) => {
                                        error!("Error processing command: {:?}", err);
                                    }
                                }
                            }
                            Some(IncomingCommand::StateInit(cmd)) => {
                                let t0 = std::time::Instant::now();
                                match worker_use_case.execute_state_init(cmd) {
                                    Ok(id) => {
                                        info!("StateInit {} processed in {:?}", id, t0.elapsed());
                                    }
                                    Err(err) => {
                                        error!("Error processing state-init: {:?}", err);
                                    }
                                }
                            }
                            None => {
                                error!("Failed to deserialize command");
                                error!("Payload: {:?}", String::from_utf8_lossy(payload));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        error!("Kafka error: {}", e);
                    }
                    None => {}
                }
            }

            debug!("Unsubscribing...");
            consumer.unsubscribe();
            drop(consumer);
            debug!("Consumer shutdown complete");
        })
    }
}

/// Try to deserialize as HsmWorkerRequestDto first, then fall back to StateInitCommandDto.
fn deserialize_command(payload: &[u8]) -> Option<IncomingCommand> {
    // Try HsmWorkerRequestDto first (has outer_request_jws field)
    if let Ok(dto) = serde_json::from_slice::<HsmWorkerRequestDto>(payload) {
        let request = HsmWorkerRequest {
            correlation_id: dto.correlation_id,
            device_id: dto.device_id,
            request_id: dto.request_id,
            state_version: dto.state_version,
            outer_request_jws: dto.outer_request_jws,
        };
        return Some(IncomingCommand::WorkerRequest(request));
    }

    // Fall back to StateInitCommandDto
    if let Ok(cmd) = serde_json::from_slice::<StateInitCommandDto>(payload) {
        return Some(IncomingCommand::StateInit(cmd));
    }

    None
}
