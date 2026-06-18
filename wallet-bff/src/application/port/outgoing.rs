// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::domain::{HsmWorkerRequest, RequestContext, StateInitRequest};

/// SPI port: load and save device state (JWS) in the state store.
#[async_trait::async_trait]
pub trait DeviceStatePort: Send + Sync {
    async fn save(&self, key: &str, state: &str, ttl_seconds: u64);
    async fn load(&self, key: &str) -> Option<String>;
}

/// SPI port: send worker requests to the hsm-worker request topic.
#[async_trait::async_trait]
pub trait RequestSenderPort: Send + Sync {
    async fn send(&self, request: &HsmWorkerRequest, device_id: &str) -> Result<(), String>;
}

/// SPI port: send state-init requests to the hsm-worker state-init topic.
#[async_trait::async_trait]
pub trait StateInitSenderPort: Send + Sync {
    async fn send(&self, request: &StateInitRequest, device_id: &str) -> Result<(), String>;
}

/// SPI port: replay-attack nonce store.
#[async_trait::async_trait]
pub trait NoncePort: Send + Sync {
    /// Attempt to store a nonce. Returns `true` if the nonce was new (stored
    /// successfully), `false` if it already exists (replay). Errors indicate
    /// a store connectivity problem.
    async fn try_store(
        &self,
        client_id: &str,
        nonce: &str,
        ttl_seconds: u64,
    ) -> Result<bool, String>;
}

/// SPI port: durable, multi-instance response store backing the async flow.
///
/// The Kafka response consumers call [`put`] to publish the worker response;
/// HTTP GET handlers call [`await_value`] to long-poll for it. Any BFF replica
/// can serve the GET regardless of which replica's consumer received the
/// response from Kafka.
#[async_trait::async_trait]
pub trait ResponseStorePort: Send + Sync {
    /// Store the serialized response under `key` with `ttl_seconds` TTL and
    /// wake any waiters blocked in [`await_value`].
    async fn put(&self, key: &str, value: &[u8], ttl_seconds: u64) -> Result<(), String>;

    /// Return the value if already present, otherwise block up to
    /// `timeout_seconds` waiting for a [`put`]. Returns `Ok(None)` on timeout.
    async fn await_value(
        &self,
        key: &str,
        timeout_seconds: u64,
    ) -> Result<Option<Vec<u8>>, String>;
}

/// SPI port: persists the `{request_id -> (client_id, ttl)}` context an
/// in-flight Kafka request needs in order to attribute its response to a
/// device when it eventually arrives. Required because responses may be
/// consumed by a different BFF replica than the one that issued the request.
#[async_trait::async_trait]
pub trait RequestContextPort: Send + Sync {
    async fn store(
        &self,
        request_id: &str,
        ctx: &RequestContext,
        ttl_seconds: u64,
    ) -> Result<(), String>;

    /// Remove and return the context (so duplicate Kafka deliveries are no-ops).
    async fn take(&self, request_id: &str) -> Option<RequestContext>;
}
