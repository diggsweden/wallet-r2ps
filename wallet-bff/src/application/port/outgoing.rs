// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use tokio::sync::oneshot;

use crate::domain::{HsmWorkerRequest, StateInitRequest, StateInitResponse};

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

/// SPI port: register and correlate state-init requests with their responses.
#[async_trait::async_trait]
pub trait StateInitCorrelationPort: Send + Sync {
    /// Register a pending state-init request. Returns a receiver that resolves
    /// when the response arrives (or is dropped on timeout/cleanup).
    async fn register_pending(
        &self,
        request_id: &str,
        state_key: &str,
        ttl_seconds: u64,
    ) -> oneshot::Receiver<StateInitResponse>;

    /// Called by the Kafka consumer when a response arrives.
    async fn response_received(&self, response: StateInitResponse);
}
