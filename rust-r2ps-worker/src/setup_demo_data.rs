use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use josekit::jwk::Jwk;
use pem::Pem;
use rdkafka::message::DeliveryResult;
use rdkafka::producer::{BaseProducer, BaseRecord, Producer, ProducerContext};
use rdkafka::{ClientConfig, ClientContext, Message};
use ring::signature::{Ed25519KeyPair, KeyPair};
use rust_r2ps_worker::application::permit_list_use_case::{DeviceKey, PermitListDto, PermitStatus};
use rust_r2ps_worker::infrastructure::KafkaConfig;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{error, info, instrument, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};
use uuid::Uuid;

struct ProducerCallbackContext;

impl ClientContext for ProducerCallbackContext {}

impl ProducerContext for ProducerCallbackContext {
    type DeliveryOpaque = ();

    fn delivery(
        &self,
        delivery_result: &DeliveryResult<'_>,
        _delivery_opaque: Self::DeliveryOpaque,
    ) {
        match delivery_result {
            Ok(message) => {
                info!(
                    "Message delivered to topic '{}' partition [{}] at offset {}",
                    message.topic(),
                    message.partition(),
                    message.offset()
                );
            }
            Err((err, message)) => {
                error!(
                    "Failed to deliver message to topic '{}': {:?}",
                    message.topic(),
                    err
                );
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = KafkaConfig::init().unwrap();

    // init tracing
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_thread_ids(true) // Include thread IDs
                .with_thread_names(true) // Include thread names
                .with_target(false) // Hide target (module path)
                .with_level(true), // Show log levels
        )
        .with(
            // Filter based on RUST_LOG env var, default to info
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("bootstrap.servers {}", &config.bootstrap_servers);

    let context = ProducerCallbackContext;
    let producer: BaseProducer<ProducerCallbackContext> = ClientConfig::new()
        .set("bootstrap.servers", &config.bootstrap_servers)
        .set("broker.address.family", &config.broker_address_family)
        .set("message.timeout.ms", "5000")
        .create_with_context(context)
        .expect("Producer creation failed");

    let rng = ring::rand::SystemRandom::new();

    for device_no in 1..10 {
        let device_id = Uuid::new_v4();
        let server_wallet_id = Uuid::new_v4();

        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rng)?;

        // Create the key pair from PKCS#8
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref())?;

        // === Convert Private Key to PEM ===
        let private_pem = Pem::new("PRIVATE KEY", pkcs8_bytes.as_ref().to_vec());
        let private_pem_string = pem::encode(&private_pem);

        // === Convert Public Key to PEM ===
        // Ed25519 public key in SubjectPublicKeyInfo (SPKI) format
        let public_key_bytes = key_pair.public_key().as_ref();

        // Create SPKI wrapper for Ed25519 public key
        // OID for Ed25519: 1.3.101.112
        let spki = create_ed25519_spki(public_key_bytes);

        let public_pem = Pem::new("PUBLIC KEY", spki);
        let public_pem_string = pem::encode(&public_pem);

        println!("=== Public Key (PEM) ===");
        println!("{}", public_pem_string);

        // === Convert Public Key to JWK using josekit ===
        let jwk = ed25519_public_key_to_jwk(public_key_bytes)?;

        println!("=== Public Key (JWK) ===");
        println!("{}", serde_json::to_string_pretty(&jwk)?);

        let permit_device = PermitListDto {
            device_id,
            server_wallet_id,
            device_keys: PermitStatus::Allow(DeviceKey {
                device_public_key: jwk,
            }),
        };

        info!(
            "Demo client data: {:?} {}",
            permit_device, private_pem_string
        );

        match serde_json::to_string(&permit_device) {
            Ok(json) => {
                let key = permit_device.device_id.to_string();
                let record = BaseRecord::to("wallet-permit-list")
                    .key(&key)
                    .payload(&json);

                match producer.send(record) {
                    Ok(_) => {
                        // Message enqueued successfully
                        info!("Message sent: payload='{}'", json);
                    }
                    Err((err, _)) => {
                        error!("Failed to send message: {:?}", err);
                    }
                }
            }
            Err(err) => {}
        }
        // Poll to trigger delivery callbacks (non-blocking)
        producer.poll(Duration::from_millis(0));
    }

    // Flush remaining messages and wait for all delivery callbacks
    info!("Flushing producer...");
    producer.flush(Duration::from_secs(10))?;

    info!("All messages delivered");
    Ok(())
}

fn create_ed25519_spki(public_key: &[u8]) -> Vec<u8> {
    // SPKI structure for Ed25519:
    // SEQUENCE {
    //   SEQUENCE {
    //     OBJECT IDENTIFIER 1.3.101.112 (Ed25519)
    //   }
    //   BIT STRING (public key)
    // }

    // Ed25519 OID: 1.3.101.112 -> 06 03 2B 65 70
    let algorithm_identifier: &[u8] = &[
        0x30, 0x05, // SEQUENCE, length 5
        0x06, 0x03, // OBJECT IDENTIFIER, length 3
        0x2B, 0x65, 0x70, // 1.3.101.112 (Ed25519)
    ];

    // BIT STRING wrapper for public key (32 bytes for Ed25519)
    // 03 = BIT STRING tag, 21 = length (33 = 1 + 32), 00 = unused bits
    let bit_string_header: &[u8] = &[0x03, 0x21, 0x00];

    // Calculate total length
    let inner_length = algorithm_identifier.len() + bit_string_header.len() + public_key.len();

    let mut spki = Vec::with_capacity(2 + inner_length);
    spki.push(0x30); // SEQUENCE tag
    spki.push(inner_length as u8); // length
    spki.extend_from_slice(algorithm_identifier);
    spki.extend_from_slice(bit_string_header);
    spki.extend_from_slice(public_key);

    spki
}

/// Convert Ed25519 raw public key bytes to a josekit JWK
fn ed25519_public_key_to_jwk(public_key: &[u8]) -> Result<Jwk, Box<dyn std::error::Error>> {
    // Create JWK manually for Ed25519 (OKP key type)
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();

    // Key type: OKP (Octet Key Pair) for EdDSA keys
    params.insert(
        "kty".to_string(),
        serde_json::Value::String("OKP".to_string()),
    );

    // Curve: Ed25519
    params.insert(
        "crv".to_string(),
        serde_json::Value::String("Ed25519".to_string()),
    );

    // x: the public key (base64url encoded)
    let x = URL_SAFE_NO_PAD.encode(public_key);
    params.insert("x".to_string(), serde_json::Value::String(x));

    // Optional: key use (signature)
    params.insert(
        "use".to_string(),
        serde_json::Value::String("sig".to_string()),
    );

    // Convert to JWK
    let jwk_json = serde_json::to_string(&params)?;
    let jwk = Jwk::from_bytes(jwk_json.as_bytes())?;

    Ok(jwk)
}
