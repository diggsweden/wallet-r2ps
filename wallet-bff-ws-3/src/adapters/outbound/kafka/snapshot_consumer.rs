//! State-snapshot Kafka consumer.
//!
//! Consumes the log-compacted `state-snapshot` topic which contains
//! HSM-signed JWS tokens (ES256) of `DeviceHsmState`, keyed by `client_id`.
//! Extracts the device public keys from each snapshot and stores them in
//! Valkey for WebSocket authentication.
//!
//! The JWS payload is parsed without signature verification — the BFF
//! trusts the Kafka transport and the worker's signing authority.
//!
//! This consumer uses the main consumer group (not per-pod unique) so that
//! each snapshot is processed exactly once across the BFF cluster. The
//! Valkey key cache is shared across all pods.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rdkafka::{
    ClientConfig, Message,
    consumer::{Consumer, StreamConsumer},
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::config::KafkaConfig;
use crate::domain::device_management::value_objects::{ClientId, EcPublicKeyData};
use crate::ports::outbound::ClientKeyRepository;

/// Snapshot of `DeviceHsmState` extracted from the JWS payload.
///
/// The worker signs the full `DeviceHsmState` as a JWS; we only need
/// the `device_keys` field for populating the Valkey key cache.
#[derive(Debug, Deserialize)]
struct StateSnapshotMessage {
    /// All device key entries from the HSM state.
    device_keys: Vec<SnapshotDeviceKeyEntry>,
}

/// A device key entry within a state snapshot.
#[derive(Debug, Deserialize)]
struct SnapshotDeviceKeyEntry {
    public_key: SnapshotEcPublicKey,
}

/// EC public key data from a snapshot.
#[derive(Debug, Deserialize)]
struct SnapshotEcPublicKey {
    kty: String,
    crv: String,
    x: String,
    y: String,
    kid: String,
}

/// Kafka consumer for the `state-snapshot` topic.
///
/// Extracts device public keys from each snapshot and stores them in
/// Valkey via the `ClientKeyRepository` port.
pub struct StateSnapshotConsumer<K>
where
    K: ClientKeyRepository,
{
    consumer: StreamConsumer,
    key_repo: K,
}

impl<K> StateSnapshotConsumer<K>
where
    K: ClientKeyRepository + 'static,
{
    pub fn new(
        config: &KafkaConfig,
        key_repo: K,
    ) -> Result<Self, crate::domain::hsm_integration::errors::HsmError> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("broker.address.family", &config.broker_address_family)
            .set("group.id", &config.consumer.group_id)
            .set("enable.auto.commit", "true")
            // Start from the beginning to populate the cache on startup
            .set("auto.offset.reset", "earliest")
            .create()
            .map_err(|e| {
                crate::domain::hsm_integration::errors::HsmError::SendFailed(format!(
                    "Failed to create snapshot Kafka consumer: {}",
                    e
                ))
            })?;

        let topics = vec![config.consumer.state_snapshot_topic.as_str()];
        consumer.subscribe(&topics).map_err(|e| {
            crate::domain::hsm_integration::errors::HsmError::SendFailed(format!(
                "Failed to subscribe to state-snapshot topic: {}",
                e
            ))
        })?;

        Ok(Self { consumer, key_repo })
    }

    /// Start consuming snapshots. Runs until the consumer is dropped.
    pub async fn start(self: Arc<Self>) {
        use futures::StreamExt;

        info!("Starting state-snapshot Kafka consumer");

        let stream = self.consumer.stream();
        tokio::pin!(stream);

        while let Some(message_result) = stream.next().await {
            match message_result {
                Ok(message) => {
                    if let Err(e) = self.handle_message(&message).await {
                        error!("Error handling state-snapshot message: {}", e);
                    }
                }
                Err(e) => {
                    error!("State-snapshot Kafka consumer error: {}", e);
                }
            }
        }
    }

    async fn handle_message(
        &self,
        message: &impl Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = match message.payload() {
            Some(p) => p,
            None => {
                warn!("Received state-snapshot message with no payload");
                return Ok(());
            }
        };

        // The Kafka message key is the client_id
        let client_id_str = match message.key() {
            Some(k) => std::str::from_utf8(k)?,
            None => {
                warn!("Received state-snapshot message with no key (client_id)");
                return Ok(());
            }
        };

        let client_id = ClientId::new(client_id_str)
            .map_err(|e| format!("invalid client_id in snapshot key: {}", e))?;

        // The outbox relay publishes the JSONB payload as text.
        // Since we stored a serde_json::Value::String, the payload is a JSON-quoted
        // JWS compact string: "eyJhbGci..."
        let jws_compact: String = serde_json::from_slice(payload)
            .map_err(|e| format!("failed to parse snapshot payload as JSON string: {}", e))?;

        // Extract the JWS payload (middle section of header.payload.signature)
        // without verifying the signature — we trust the Kafka transport.
        let snapshot = parse_jws_payload(&jws_compact)?;

        // Extract all public keys from the snapshot
        let keys: Vec<EcPublicKeyData> = snapshot
            .device_keys
            .into_iter()
            .map(|entry| EcPublicKeyData {
                kty: entry.public_key.kty,
                crv: entry.public_key.crv,
                x: entry.public_key.x,
                y: entry.public_key.y,
                kid: entry.public_key.kid,
            })
            .collect();

        // Store in Valkey
        self.key_repo
            .store_keys(&client_id, &keys)
            .await
            .map_err(|e| format!("failed to store client keys: {}", e))?;

        info!(
            client_id = %client_id_str,
            key_count = keys.len(),
            "Updated client key cache from state snapshot"
        );

        Ok(())
    }
}

/// Parse the JWS payload section without signature verification.
///
/// A JWS compact serialization has the form: `base64url(header).base64url(payload).base64url(signature)`.
/// We extract and base64url-decode the payload section, then deserialize it as JSON.
fn parse_jws_payload(
    jws_compact: &str,
) -> Result<StateSnapshotMessage, Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = jws_compact.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "invalid JWS compact format: expected 3 parts, got {}",
            parts.len()
        )
        .into());
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| format!("failed to base64url-decode JWS payload: {}", e))?;

    let snapshot: StateSnapshotMessage = serde_json::from_slice(&payload_bytes)
        .map_err(|e| format!("failed to deserialize JWS payload as DeviceHsmState: {}", e))?;

    Ok(snapshot)
}
