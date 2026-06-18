// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! [`BackendClient`] implementation that talks directly to the worker's
//! request topics and listens on a per-process response topic — bypassing the
//! BFF entirely.
//!
//! Concurrency model:
//!   * one tuned [`FutureProducer`] shared by every virtual user (rdkafka is
//!     internally multiplexed, so contention is not on the producer)
//!   * one [`StreamConsumer`] task per response topic
//!     (hsm-worker / state-init), routing incoming messages to per-request
//!     `oneshot::Sender` slots stored in a [`DashMap`]
//!
//! A request is correlated by its `requestId` (a fresh UUID per call). The
//! caller's await suspends on the `oneshot::Receiver`; the consumer task
//! resolves it from the matching response.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use hsm_common::{
    EcPublicJwk, HsmWorkerRequest, HsmWorkerResponse, OuterRequest, StateInitRequest,
    StateInitResponse, Status, TypedJws,
};
use integration_load_tests::backend::BackendClient;
use integration_load_tests::protocol::types::{
    BffNewStateRequest, BffNewStateResponse, BffSyncResponse,
};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;
use uuid::Uuid;

use super::consumer::ResponseMap;

/// Shared device-state cache so subsequent `submit_request` calls for the
/// same `client_id` send the latest `state_jws` returned by the worker.
/// The state field is mutated on every worker response — without that the
/// worker would reject a stale state.
type StateCache = Arc<DashMap<String, String>>;

pub struct KafkaBackend {
    producer: FutureProducer,
    hsm_response_topic: String,
    state_init_response_topic: String,
    hsm_pending: ResponseMap<HsmWorkerResponse>,
    state_init_pending: ResponseMap<StateInitResponse>,
    state_cache: StateCache,
    request_timeout: Duration,
}

pub const HSM_REQUESTS_TOPIC: &str = "hsm-requests";
pub const STATE_INIT_REQUESTS_TOPIC: &str = "state-init-requests";

/// Producer tuning knobs exposed to the CLI. Defaults match the baseline
/// load-test config; aggressive runs override them.
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    pub linger_ms: u32,
    pub batch_size_bytes: u32,
    pub compression: String,
    pub queue_max_messages: u32,
    pub queue_max_kbytes: u32,
    /// `0`, `1`, or `all`. Default `1` (leader-only ack). `0` skips the
    /// broker ack entirely — for pure producer-throughput tests where the
    /// response oneshot is the only completion signal we care about.
    pub acks: String,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            linger_ms: 2,
            batch_size_bytes: 131_072,
            compression: "lz4".to_string(),
            queue_max_messages: 1_000_000,
            queue_max_kbytes: 262_144,
            acks: "1".to_string(),
        }
    }
}

impl KafkaBackend {
    /// Build a tuned producer.
    ///
    /// Defaults match what the in-cluster wallet-bff does for HSM traffic;
    /// `cfg` overrides them. Acks stay at `1` (leader-only) because this is
    /// load generation — replication durability isn't under test.
    pub fn build_producer(
        bootstrap_servers: &str,
        broker_address_family: &str,
        cfg: &ProducerConfig,
    ) -> Result<FutureProducer> {
        let p: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("broker.address.family", broker_address_family)
            .set("linger.ms", cfg.linger_ms.to_string())
            .set("batch.size", cfg.batch_size_bytes.to_string())
            .set("compression.type", &cfg.compression)
            .set("acks", &cfg.acks)
            .set("enable.idempotence", "false")
            .set("queue.buffering.max.messages", cfg.queue_max_messages.to_string())
            .set("queue.buffering.max.kbytes", cfg.queue_max_kbytes.to_string())
            .set("message.timeout.ms", "30000")
            .create()
            .context("Failed to build FutureProducer")?;
        Ok(p)
    }

    pub fn new(
        producer: FutureProducer,
        hsm_response_topic: String,
        state_init_response_topic: String,
        hsm_pending: ResponseMap<HsmWorkerResponse>,
        state_init_pending: ResponseMap<StateInitResponse>,
        state_cache: StateCache,
        request_timeout: Duration,
    ) -> Self {
        Self {
            producer,
            hsm_response_topic,
            state_init_response_topic,
            hsm_pending,
            state_init_pending,
            state_cache,
            request_timeout,
        }
    }

    /// Manually pre-populate the state cache. Used by the load-test command
    /// to seed clients whose state was captured during a previous generate
    /// run.
    pub fn seed_state(&self, client_id: &str, state_jws: &str) {
        self.state_cache
            .insert(client_id.to_string(), state_jws.to_string());
    }

    /// Read the current `state_jws` for `client_id` out of the cache. Used
    /// by the generate command to persist final state into the dataset.
    pub fn snapshot_state(&self, client_id: &str) -> Option<String> {
        self.state_cache.get(client_id).map(|v| v.value().clone())
    }

    /// Fire-and-forget produce of a `HsmWorkerRequest`. No correlation slot
    /// is registered, so the worker's eventual response is dropped as an
    /// orphan. Used by `--produce-only` benchmarks that just want to push
    /// messages at line rate without waiting on the response cycle.
    pub fn fire_hsm_request(&self, client_id: &str, outer_request_jws: &str) -> Result<()> {
        let state_jws = self
            .state_cache
            .get(client_id)
            .map(|s| s.value().clone())
            .ok_or_else(|| anyhow!("No cached state_jws for client_id {client_id}"))?;
        let envelope = HsmWorkerRequest {
            request_id: Uuid::new_v4().to_string(),
            state_jws,
            outer_request_jws: TypedJws::<OuterRequest>::new(outer_request_jws.to_string()),
            response_topic: self.hsm_response_topic.clone(),
        };
        let payload = serde_json::to_vec(&envelope).context("serialise HsmWorkerRequest")?;
        let record: FutureRecord<'_, str, [u8]> = FutureRecord::to(HSM_REQUESTS_TOPIC)
            .key(client_id)
            .payload(&payload);
        self.producer
            .send_result(record)
            .map_err(|(e, _)| anyhow!("kafka enqueue failed: {e}"))?;
        Ok(())
    }
}

#[async_trait]
impl BackendClient for KafkaBackend {
    async fn submit_request(
        &self,
        client_id: &str,
        outer_request_jws: &str,
    ) -> Result<BffSyncResponse> {
        let request_id = Uuid::new_v4().to_string();
        let state_jws = self
            .state_cache
            .get(client_id)
            .map(|s| s.value().clone())
            .ok_or_else(|| {
                anyhow!(
                    "No cached state_jws for client_id {client_id} — call create_device_state first"
                )
            })?;

        let outer = TypedJws::<OuterRequest>::new(outer_request_jws.to_string());

        let envelope = HsmWorkerRequest {
            request_id: request_id.clone(),
            state_jws,
            outer_request_jws: outer,
            response_topic: self.hsm_response_topic.clone(),
        };

        let (tx, rx) = oneshot::channel();
        self.hsm_pending.insert(request_id.clone(), tx);

        let payload = serde_json::to_vec(&envelope).context("serialise HsmWorkerRequest")?;
        // Use client_id as partition key so worker side keeps in-order
        // processing per device — same scheme the BFF uses.
        let record: FutureRecord<'_, str, [u8]> = FutureRecord::to(HSM_REQUESTS_TOPIC)
            .key(client_id)
            .payload(&payload);
        // Fire-and-forget enqueue. We don't await broker ack; the response
        // oneshot is the real completion signal. With many concurrent VUs
        // this lets `linger.ms` actually coalesce messages into big batches
        // instead of every task serialising on its own ack.
        self.producer.send_result(record).map_err(|(e, _)| {
            self.hsm_pending.remove(&request_id);
            anyhow!("kafka enqueue failed: {e}")
        })?;

        let resp = match timeout(self.request_timeout, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => {
                anyhow::bail!("response slot dropped for {request_id}");
            }
            Err(_) => {
                self.hsm_pending.remove(&request_id);
                anyhow::bail!("timeout waiting for response to {request_id}");
            }
        };

        if let Some(new_state) = resp.state_jws.as_ref() {
            self.state_cache
                .insert(client_id.to_string(), new_state.clone());
        }

        let status_str = match resp.status {
            Status::Ok => "complete",
            Status::Error => "error",
        }
        .to_string();
        let result_jws = resp.outer_response_jws.map(|j| j.into_string());

        if matches!(resp.status, Status::Error) {
            anyhow::bail!(
                "worker error: {}",
                resp.error_message.unwrap_or_else(|| "<no message>".into())
            );
        }

        Ok(BffSyncResponse {
            correlation_id: resp.request_id,
            status: status_str,
            result: result_jws,
            result_url: None,
            error: None,
        })
    }

    async fn create_device_state(
        &self,
        request: &BffNewStateRequest,
    ) -> Result<BffNewStateResponse> {
        let request_id = Uuid::new_v4().to_string();
        // Each load-test client owns its own client_id from the moment of
        // state-init. We mint it locally and key the partition by it so the
        // worker processes all subsequent requests for this device in order.
        let client_id = Uuid::new_v4().to_string();

        let envelope = StateInitRequest {
            request_id: request_id.clone(),
            public_key: EcPublicJwk {
                kty: request.public_key.kty.clone(),
                crv: request.public_key.crv.clone(),
                x: request.public_key.x.clone(),
                y: request.public_key.y.clone(),
                kid: request.public_key.kid.clone(),
            },
            response_topic: self.state_init_response_topic.clone(),
            initial_key_curve: hsm_common::Curve::P256,
        };

        let (tx, rx) = oneshot::channel();
        self.state_init_pending.insert(request_id.clone(), tx);

        let payload = serde_json::to_vec(&envelope).context("serialise StateInitRequest")?;
        let record: FutureRecord<'_, str, [u8]> = FutureRecord::to(STATE_INIT_REQUESTS_TOPIC)
            .key(client_id.as_str())
            .payload(&payload);
        self.producer.send_result(record).map_err(|(e, _)| {
            self.state_init_pending.remove(&request_id);
            anyhow!("kafka enqueue failed: {e}")
        })?;

        let resp = match timeout(self.request_timeout, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => {
                anyhow::bail!("response slot dropped for {request_id}");
            }
            Err(_) => {
                self.state_init_pending.remove(&request_id);
                anyhow::bail!("timeout waiting for state-init response {request_id}");
            }
        };

        self.state_cache
            .insert(client_id.clone(), resp.state_jws.clone());

        Ok(BffNewStateResponse {
            status: "OK".to_string(),
            client_id,
            dev_authorization_code: Some(resp.dev_authorization_code),
        })
    }
}

/// State cache constructor — exposed so the binary can share the same map
/// across multiple [`KafkaBackend`] instances if needed.
pub fn new_state_cache() -> StateCache {
    Arc::new(DashMap::new())
}
