
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::producer::{BaseProducer, BaseRecord};
use rdkafka::message::Message;
use std::time::Duration;
use jws::compact::{decode_unverified};
use serde::{Deserialize, Serialize};
use serde_json::from_slice;
use tracing::{debug, error, info, instrument, warn};
use tracing_subscriber::{fmt, EnvFilter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use dotenv_config::EnvConfig;
use dotenvy::dotenv;
use foyer::{Cache, CacheBuilder, EvictionConfig, LruConfig};

#[derive(Debug, EnvConfig)]
struct KafkaConfig {
    #[env_config(name="BOOTSTRAP_SERVERS", default = "127.0.0.1:9092")]
    bootstrap_servers: String,

    #[env_config(default = "v4")]
    broker_address_family: String,


    #[env_config(name="GROUP_ID", default = "rust-grp")]
    group_id: String,

    #[env_config(name="GROUP_INSTANCE_ID", default = "consumer-1")]
    group_instance_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct R2psRequestDto {
    pub request_id: String,
    pub wallet_id: String,
    pub device_id: String,
    pub payload: String,
}

// Define your output message structure
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct R2PsResponseDto {
    pub request_id: String,
    pub wallet_id: String,
    pub device_id: String,
    pub status: u16,
    pub payload: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServiceRequest {
    pub client_id: String,
    pub kid: String,
    pub context: String,
    #[serde(rename = "type")]
    pub service_type: String,
    pub pake_session_id: Option<String>,
    #[serde(rename = "ver")]
    pub version: Option<String>,
    pub nonce: Option<String>,
    pub iat: Option<i64>,
    pub enc: Option<String>,
    #[serde(rename = "data")]
    pub service_data: Option<String>,
}



#[instrument(name="main")]
fn main() {

    let cache: Cache<String, String> = CacheBuilder::new(2048)
        .with_eviction_config(EvictionConfig::Lru(LruConfig {
            high_priority_pool_ratio: 0.8,
        }))
        .build();

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_thread_ids(true)      // Include thread IDs
                .with_thread_names(true)    // Include thread names
                .with_target(false)         // Hide target (module path)
                .with_level(true)
                // Show log levels
        )
        .with(
            // Filter based on RUST_LOG env var, default to info
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    dotenv().ok();
    let cfg = KafkaConfig::init().unwrap();

    let help = KafkaConfig::get_help();
    info!("{:#?}", help);

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Handle Ctrl+C
    ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        r.store(false, Ordering::Relaxed);
    }).expect("Error setting Ctrl-C handler");

    info!("HELLO");
    // Configure consumer
    let consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", &cfg.bootstrap_servers)
        .set("broker.address.family", &cfg.broker_address_family)
        .set("group.id", &cfg.group_id)
        .set("group.instance.id", &cfg.group_instance_id)
        .set("enable.auto.commit", "true")
        .set("auto.offset.reset", "earliest")
        .set("fetch.wait.max.ms", "500")
        .set("session.timeout.ms", "6000")           // Default: 45000ms
        .set("heartbeat.interval.ms", "2000")        // Default: 3000ms
        .set("max.poll.interval.ms", "300000")
        .set("connections.max.idle.ms", "540000")
        .set("metadata.max.age.ms", "5000")
        .set("partition.assignment.strategy", "cooperative-sticky")// Default: 300000ms
        .create()
        .expect("Consumer creation failed");

    // Subscribe to input topic
    consumer
        .subscribe(&["r2ps-requests"])
        .expect("Failed to subscribe to topic");

    // Configure producer (synchronous)
    let producer: BaseProducer = ClientConfig::new()
        .set("bootstrap.servers", &cfg.bootstrap_servers)
        .set("broker.address.family", &cfg.broker_address_family)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("Producer creation failed");

    info!("Starting Kafka consumer-producer pipeline...");

    // Main processing loop
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

                let input_msg: R2psRequestDto = match from_slice(payload) {
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
                let output_msg = process_message(&cache, input_msg);

                // Serialize output message to JSON
                let output_json = match serde_json::to_string(&output_msg) {
                    Ok(json) => json,
                    Err(e) => {
                        error!("Failed to serialize output message: {:?}", e);
                        continue;
                    }
                };

                // Send to output topic
                let output_key = format!("processed-{:?}", key);
                let key = output_msg.request_id.clone();
                let record = BaseRecord::to("output-topic")
                    .key(&key)
                    .payload(&output_json);

                match producer.send(record) {
                    Ok(_) => {
                        // Message enqueued successfully
                        info!("Message sent: key='{}'", output_key);
                    }
                    Err((err, _)) => {
                        error!("Failed to send message: {:?}", err);
                    }
                }

                // Poll producer to handle delivery reports and callbacks
                producer.poll(Duration::from_millis(100));
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
}


#[instrument(name = "worker")]
fn process_message(cache: &Cache<String, String>, input: R2psRequestDto) -> R2PsResponseDto {
    // Transform the message (example: convert to uppercase and add prefix)
    debug!("Received message: {:?}", input);

    // Decode and verify the message.
    match decode_unverified(input.payload.as_bytes()) {
        Ok((message, signature)) => {
            debug!("Decoded JWS");

            let _req : Option<ServiceRequest>  =  match serde_json::from_slice::<ServiceRequest>(&message.payload.to_vec()) {
                Ok(msg) => {
                    info!("deserialized jws payload message: {:?}", msg);

                    match msg.context == "hsm" {
                        true => match &msg.pake_session_id {
                            Some(session_id) => {
                                Some(msg)
                            },
                            None => None // opaque
                        } ,
                        false => None
                    }
                },
                Err(e) => {
                    error!("Failed to deserialize JSON: {:?}", e);
                    error!("Payload: {:?}", String::from_utf8_lossy(&message.payload));
                    None
                }
            };

        },
        Err(e) => {
            error!("Failed to decode unverified message: {:?}", e);
        },
    }

    R2PsResponseDto{
        request_id: input.request_id.to_string(),
        wallet_id: input.wallet_id.to_string(),
        device_id: input.device_id.to_string(),
        status: 200,
        payload: format!("PROCESSED: {}", input.payload.to_uppercase()),
    }
}