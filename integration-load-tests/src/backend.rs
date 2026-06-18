// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Backend abstraction over the wallet-bff request/response flow. Both the REST
//! backend (BFF over HTTP) and a direct-Kafka backend implement this trait so
//! the high-level [`AccessMechanismClient`](crate::client::access_mechanism)
//! code can be reused across load-test tools.

use anyhow::Result;
use async_trait::async_trait;

use crate::protocol::types::{BffNewStateRequest, BffNewStateResponse, BffSyncResponse};

#[async_trait]
pub trait BackendClient: Send + Sync {
    /// Send an `OuterRequest` JWS for `client_id` and wait for the response
    /// envelope. The returned [`BffSyncResponse`] mirrors the BFF REST shape;
    /// Kafka backends fill in `result` from `outerResponseJws` and leave
    /// `result_url` / `correlation_id` empty.
    async fn submit_request(
        &self,
        client_id: &str,
        outer_request_jws: &str,
    ) -> Result<BffSyncResponse>;

    /// Initialise a device state for a new client. Returns the assigned
    /// `client_id` and the one-time `dev_authorization_code` used during
    /// OPAQUE PIN registration.
    async fn create_device_state(
        &self,
        request: &BffNewStateRequest,
    ) -> Result<BffNewStateResponse>;
}
