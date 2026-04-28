// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use tokio::sync::oneshot;

use crate::domain::{CachedResponse, HsmWorkerResponse};

/// Use case port for handling incoming worker responses.
#[async_trait::async_trait]
pub trait ResponseUseCase: Send + Sync {
    /// Register a pending hsm-worker request and return a receiver that fires
    /// when the worker response arrives. Await the receiver with a timeout in
    /// the HTTP handler.
    fn register_pending(
        &self,
        request_id: &str,
        state_key: &str,
        ttl_seconds: u64,
    ) -> oneshot::Receiver<CachedResponse>;

    /// Called by the Kafka consumer when a response arrives.
    fn response_ready(&self, response: HsmWorkerResponse);

    /// Polling path (GET /hsm/v1/requests/{id}): returns a cached response if
    /// already completed, or waits up to `timeout_ms` for it to arrive.
    async fn wait_for_response(&self, request_id: &str, timeout_ms: u64) -> Option<CachedResponse>;
}
