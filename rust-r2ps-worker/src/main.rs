use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::producer::{BaseProducer, BaseRecord};
use rdkafka::message::Message;
use std::time::Duration;
use opaque_ke::argon2::Argon2;
use base64::Engine;
use base64::engine::general_purpose;
use base64::prelude::BASE64_STANDARD;
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
use josekit::jwe::{JweDecrypter, JweHeader, ECDH_ES};
use josekit::jwe::enc::{A128GCM, A192GCM, A256GCM, A128CBC_HS256, A192CBC_HS384, A256CBC_HS512};
use josekit::jwk::Jwk;
use josekit::jwt::JwtPayload;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use opaque_ke::ciphersuite::CipherSuite;
use opaque_ke::{RegistrationRequest, ServerRegistration, ServerRegistrationLen, ServerSetup};
use opaque_ke::generic_array::GenericArray;
use opaque_ke::ksf::Identity;
use rand::rngs::OsRng;
use sha2::Sha256;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakeResponsePayload {
    /// The PAKE session ID assigned by the server
    #[serde(rename = "pake_session_id")]
    pub pake_session_id: Option<String>,

    /// The session task recognized by the server bound to this pake session ID
    #[serde(rename = "task")]
    pub task: Option<String>,

    /// PAKE response data as defined by the PAKE state in the request
    #[serde(rename = "resp")]
    pub response_data: Option<String>,

    #[serde(rename = "msg")]
    pub message: Option<String>,

    #[serde(rename = "session_expiration_time")]
    pub session_expiration_time: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    ver: String,
    nonce: String,
    iat: i64,
    enc: String,
    data: String,
}

// The ciphersuite trait allows to specify the underlying primitives that will
// be used in the OPAQUE protocol

pub struct DefaultCipherSuite;

impl CipherSuite for DefaultCipherSuite {
    type OprfCs = p256::NistP256;
    type KeyExchange = opaque_ke::TripleDh<p256::NistP256, sha2::Sha256,>;
    type Ksf = Identity;
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PakeProtocol {
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PakeState {
    Evaluate,
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakeRequestPayload {
    /// Identifies the PAKE protocol
    #[serde(rename = "protocol")]
    pub protocol: PakeProtocol,

    /// Identifies the PAKE state which determines the data content.
    /// E.g., evaluate or finalize for OPAQUE
    #[serde(rename = "state")]
    pub state: PakeState,

    /// Optional authorization data required for initial PIN registrations or PIN resets
    #[serde(rename = "authorization", skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,

    #[serde(rename = "task", skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,

    #[serde(
        rename = "session_duration",
        skip_serializing_if = "Option::is_none",
        with = "duration_serde",
        default
    )]
    pub session_duration: Option<Duration>,

    /// The PAKE request data as defined by the PAKE state
    #[serde(rename = "req")]
    pub request_data: String,
}

impl PakeRequestPayload {
    /// Serializes the payload to bytes
    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserializes the payload from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

// Helper module for Duration serialization/deserialization
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match duration {
            Some(d) => serializer.serialize_u64(d.as_secs()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = Option::<u64>::deserialize(deserializer)?;
        Ok(secs.map(Duration::from_secs))
    }
}

#[instrument(name="main", skip_all)]
fn main() {

    let cache: Cache<String, String> = CacheBuilder::new(2048)
        .with_eviction_config(EvictionConfig::Lru(LruConfig {
            high_priority_pool_ratio: 0.8,
        }))
        .build();

    let mut rng = OsRng;

    let server_setup = ServerSetup::<DefaultCipherSuite>::new(&mut rng);

    let mut registered_users =
        HashMap::<String, GenericArray<u8, ServerRegistrationLen<DefaultCipherSuite>>>::new();
    //registered_users.insert("a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(), Default::default());
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
                let output_msg = process_message(&server_setup, &cache, input_msg);

                match (output_msg) {
                    Some(om) => {
                        // Serialize output message to JSON
                        let output_json = match serde_json::to_string(&om) {
                            Ok(json) => json,
                            Err(e) => {
                                error!("Failed to serialize output message: {:?}", e);
                                continue;
                            }
                        };

                        // Send to output topic
                        let key = om.wallet_id;
                        let request_id = om.request_id.clone();
                        let record = BaseRecord::to("r2ps-responses-rust")
                            .key(&key)
                            .payload(&output_json);

                        match producer.send(record) {
                            Ok(_) => {
                                // Message enqueued successfully
                                info!("Message sent: key='{}' request_id='{}'", key, request_id);
                            }
                            Err((err, _)) => {
                                error!("Failed to send message: {:?}", err);
                            }
                        }

                        // Poll producer to handle delivery reports and callbacks
                        producer.poll(Duration::from_millis(100));
                    }
                    None => {
                        info!("No output message");
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
}

pub fn encrypt_with_ec_jwk(
    payload: &PakeResponsePayload,
    ec_public_jwk: &josekit::jwk::Jwk,
) -> Result<String, Box<dyn std::error::Error>> {
    let payload_bytes = serde_json::to_vec(payload)?;

    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");

    let encrypter = ECDH_ES.encrypter_from_jwk(ec_public_jwk)?;
    let jwe = josekit::jwe::serialize_compact(&payload_bytes, &header, &encrypter)?;

    Ok(jwe)
}


fn decrypt_jwe_with_ecdh(jwe_token: &str, private_key_jwk: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {

    // Parse the private key from JWK format
    let jwk: Jwk = serde_json::from_str(private_key_jwk)?;

    // Parse the JWE to get header - JWE format is: header.encrypted_key.iv.ciphertext.tag
    let parts: Vec<&str> = jwe_token.split('.').collect();
    if parts.len() != 5 {
        return Err("Invalid JWE format".into());
    }

    // Decode the header
    let header_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&parts[0])?;
    //let header = JweHeader::from_bytes(&header_bytes)?;

    // Create a decrypter for ECDH-ES
    let decrypter = ECDH_ES.decrypter_from_jwk(&jwk)?;

    info!("jwk keyid: {:?} decrypter key id: {:?}", &jwk.key_id(), &decrypter.key_id());
    let (payload, header) = josekit::jwe::deserialize_compact(jwe_token, &decrypter)?;
    // Get the content encryption algorithm from the header
    let enc_algorithm = header.content_encryption()
        .ok_or("No encryption algorithm in header")?;

    // Get the appropriate content encryption
    let content_encryption = match enc_algorithm {
        "A128GCM" => &A128GCM as &dyn josekit::jwe::JweContentEncryption,
        "A192GCM" => &A192GCM as &dyn josekit::jwe::JweContentEncryption,
        "A256GCM" => &A256GCM as &dyn josekit::jwe::JweContentEncryption,
        "A128CBC-HS256" => &A128CBC_HS256 as &dyn josekit::jwe::JweContentEncryption,
        "A192CBC-HS384" => &A192CBC_HS384 as &dyn josekit::jwe::JweContentEncryption,
        "A256CBC-HS512" => &A256CBC_HS512 as &dyn josekit::jwe::JweContentEncryption,
        _ => return Err(format!("Unsupported encryption algorithm: {}", enc_algorithm).into()),
    };
    let decrypted_text = String::from_utf8(payload.clone())?;
    println!("Decrypted payload: {}", decrypted_text);
    // Decrypt the JWE token - need to pass as bytes
    //let payload = decrypter.decrypt(None, content_encryption, &header)?;

    Ok(payload.to_vec())
}



#[instrument(name = "worker", skip_all)]
fn process_message(server_setup: &ServerSetup<DefaultCipherSuite>, cache: &Cache<String, String>, input: R2psRequestDto) -> Option<R2PsResponseDto> {
    // Transform the message (example: convert to uppercase and add prefix)
    debug!("Received message: {:?}", input);

    // Decode and verify the message.
    match decode_unverified(input.payload.as_bytes()) {
        Ok((message, signature)) => {
            debug!("Decoded JWS");

            let response : Option<R2PsResponseDto>  =  match serde_json::from_slice::<ServiceRequest>(&message.payload.to_vec()) {
                Ok(msg) => {
                    info!("deserialized jws payload message: {:?}", msg);

                    match msg.context == "hsm" {
                        true => match &msg.pake_session_id {
                            Some(session_id) => {
                                info!("pake_session_id: {:?}", session_id);
                                None
                            },
                            None => match msg.service_type.as_str() {
                                "pin_registration" => {
                                    let private_key_jwk = r#"{
                                      "kty": "EC",
                                      "crv": "P-256",
                                      "x": "pzQxuXLeiPyzitMKQbUSVOD3Axb-l9LqVjs5GnYanA0",
                                      "y": "ZOAJjFE6FqSE8OV1zOPDT4W4DIaDNBVKeDkNus_7wkE",
                                      "d": "_NIIdRGO-qU2bjxTtnZuC45gAg6wZ0UGe9nCeM7wc0w"
                                    }"#;
                                    let service_data = &msg.service_data.unwrap();
                                    info!("SERVICE DATA ******* {} ", service_data);

                                    let decoded_bytes = BASE64_STANDARD.decode(&service_data).unwrap();
                                    let decoded_string = String::from_utf8(decoded_bytes).unwrap();
                                    info!("DECODED SERVICE DATA ******* {} ", decoded_string);

                                    let data = decrypt_jwe_with_ecdh(&decoded_string, private_key_jwk).unwrap();
                                    info!("device decrypted JWE payload: {:?}", data);

                                    match PakeRequestPayload::deserialize(&data) {
                                        Ok(pake_payload) => {
                                            info!("deserialized pake payload req={}", pake_payload.request_data);
                                            let rb: Vec<u8> = general_purpose::STANDARD
                                                .decode(pake_payload.request_data)
                                                .expect("Failed to decode base64");
                                            let reg_req: RegistrationRequest<DefaultCipherSuite> = match RegistrationRequest::deserialize(&rb) {
                                                Ok(reg_req) => {
                                                    info!("deserialized registration request: {:?}", reg_req);
                                                    reg_req
                                                },
                                                Err(err) => {
                                                    panic!("error decoding pake request bytes {:?}", err);
                                                }
                                            };

                                            match ServerRegistration::<DefaultCipherSuite>::start(
                                                server_setup,
                                                reg_req,
                                                msg.client_id.as_bytes()
                                            ) {
                                                Ok(d) => {
                                                    info!("START {:?}", d.message);
                                                    let msg = d.message.serialize().to_vec();
                                                    let pake_response = PakeResponsePayload{
                                                        pake_session_id: None,
                                                        task: None,
                                                        response_data: Some(general_purpose::STANDARD.encode(msg.to_vec())),
                                                        message: None,
                                                        session_expiration_time: None,
                                                    };
                                                    let client_public_key_jwk = r#"{
  "kty": "EC",
  "x5t": "GKAWKnh_4S0te8ZidMeerz6fZ2Q",
  "crv": "P-256",
  "x": "233YaUniXpEuNY15ZyJmqi-t4VtHE0BsFyM6fMWvL4w",
  "y": "bXYg-7vLtnk2ZVrCv162DwqGxEVGz2ilCfVvpdfQllA"
}"#;
                                                    let jwk: Jwk = serde_json::from_str(private_key_jwk).unwrap();
;
                                                    let jwe = encrypt_with_ec_jwk(&pake_response, &jwk).unwrap();

                                                    let jws: String = jws_with_jwk(&jwe);
                                                    let res = R2PsResponseDto{
                                                        request_id: input.request_id.to_string(),
                                                        wallet_id: input.wallet_id.to_string(),
                                                        device_id: input.device_id.to_string(),
                                                        status: 200,
                                                        payload: jws,
                                                    };
                                                    info!("RESPONSE={:?}", res);
                                                    Some(res)
                                                },
                                                Err(e) => {
                                                    error!("ERROR {:?}", e);
                                                    None
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            info!("deserialize {:?}", e);
                                            None
                                        }
                                    }
                                },
                                _ => None
                            } // opaque
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
            response
        },
        Err(e) => {
            error!("Failed to decode unverified message: {:?}", e);
            None
        },
    }
}

fn jws_with_jwk(data: &str) -> String {
    let claims = Claims {
        ver: "1.0".to_string(),
        nonce: "7c4baffd469e9285afc867242c3f569f944d56ff8684a7bc34b2b98a5312999b".to_string(),
        iat: 1763997596,
        enc: "device".to_string(),
        data: data.to_string(),
    };
    let mut header = Header::new(Algorithm::ES256);
    header.typ = Some("JOSE".to_string());

    let private_key_pem = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg/NIIdRGO+qU2bjxT
tnZuC45gAg6wZ0UGe9nCeM7wc0yhRANCAASnNDG5ct6I/LOK0wpBtRJU4PcDFv6X
0upWOzkadhqcDWTgCYxROhakhPDldczjw0+FuAyGgzQVSng5DbrP+8JB
-----END PRIVATE KEY-----"#;

    let encoding_key = EncodingKey::from_ec_pem(private_key_pem.as_bytes()).unwrap();

    let token = encode(&header, &claims, &encoding_key).unwrap();

    println!("JWS Token: {}", token);
    token
}