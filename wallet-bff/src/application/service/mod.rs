// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;
use tracing::{error, info, warn};

use crate::application::port::incoming::{HsmResponseSinkPort, StateInitResponseSinkPort};
use crate::application::port::outgoing::{DeviceStatePort, RequestContextPort, ResponseStorePort};
use crate::domain::{
    CachedResponse, CachedStateInitResponse, HsmWorkerResponse, StateInitResponse,
};

/// Redis key under which the JSON-encoded [`CachedResponse`] for a given
/// `request_id` is stored.
pub fn hsm_response_key(request_id: &str) -> String {
    format!("hsm-response:{request_id}")
}

/// Redis key under which the JSON-encoded [`CachedStateInitResponse`] for a
/// given `request_id` is stored.
pub fn state_init_response_key(request_id: &str) -> String {
    format!("state-init-response:{request_id}")
}

/// Ingests hsm-worker responses from Kafka and writes them — together with the
/// updated device state — into the durable response/state store. Replaces the
/// in-memory pending map: a poll landing on a different BFF replica than the
/// one that issued the POST can still read the response.
pub struct HsmResponseSinkService {
    device_state_port: Arc<dyn DeviceStatePort>,
    response_store: Arc<dyn ResponseStorePort>,
    request_context: Arc<dyn RequestContextPort>,
    response_ttl_seconds: u64,
}

impl HsmResponseSinkService {
    pub fn new(
        device_state_port: Arc<dyn DeviceStatePort>,
        response_store: Arc<dyn ResponseStorePort>,
        request_context: Arc<dyn RequestContextPort>,
        response_ttl_seconds: u64,
    ) -> Self {
        Self {
            device_state_port,
            response_store,
            request_context,
            response_ttl_seconds,
        }
    }
}

#[async_trait::async_trait]
impl HsmResponseSinkPort for HsmResponseSinkService {
    async fn ingest(&self, response: HsmWorkerResponse) {
        let request_id = response.request_id.clone();
        let ctx = self.request_context.take(&request_id).await;

        let cached = CachedResponse::from(response);

        // Persist the (possibly updated) device state under the original client_id.
        // The context may be missing if a stale duplicate response was delivered, in
        // which case we still publish the envelope so a slow poll can read it.
        if let (Some(state_jws), Some(ctx)) = (cached.state_jws.as_ref(), ctx.as_ref()) {
            self.device_state_port
                .save(&ctx.client_id, state_jws, ctx.ttl_seconds)
                .await;
        }

        if ctx.is_none() {
            warn!(
                "No request context for hsm-worker requestId: {} — publishing anyway",
                request_id
            );
        }

        match serde_json::to_vec(&cached) {
            Ok(bytes) => {
                if let Err(e) = self
                    .response_store
                    .put(&hsm_response_key(&request_id), &bytes, self.response_ttl_seconds)
                    .await
                {
                    error!("Failed to publish hsm response {}: {}", request_id, e);
                } else {
                    info!("Published hsm response for requestId: {}", request_id);
                }
            }
            Err(e) => error!("Failed to serialize CachedResponse {}: {}", request_id, e),
        }
    }
}

/// Ingests state-init responses from Kafka. Mirrors [`HsmResponseSinkService`]
/// but for the state-init topic.
pub struct StateInitResponseSinkService {
    device_state_port: Arc<dyn DeviceStatePort>,
    response_store: Arc<dyn ResponseStorePort>,
    request_context: Arc<dyn RequestContextPort>,
    response_ttl_seconds: u64,
}

impl StateInitResponseSinkService {
    pub fn new(
        device_state_port: Arc<dyn DeviceStatePort>,
        response_store: Arc<dyn ResponseStorePort>,
        request_context: Arc<dyn RequestContextPort>,
        response_ttl_seconds: u64,
    ) -> Self {
        Self {
            device_state_port,
            response_store,
            request_context,
            response_ttl_seconds,
        }
    }
}

#[async_trait::async_trait]
impl StateInitResponseSinkPort for StateInitResponseSinkService {
    async fn ingest(&self, response: StateInitResponse) {
        let request_id = response.request_id.clone();
        let Some(ctx) = self.request_context.take(&request_id).await else {
            warn!(
                "No request context for state-init requestId: {} — dropping",
                request_id
            );
            return;
        };

        self.device_state_port
            .save(&ctx.client_id, &response.state_jws, ctx.ttl_seconds)
            .await;

        let cached = CachedStateInitResponse {
            request_id: response.request_id.clone(),
            client_id: ctx.client_id,
            state_jws: response.state_jws,
            dev_authorization_code: response.dev_authorization_code,
            server_jws_public_key: response.server_jws_public_key,
            server_jws_kid: response.server_jws_kid,
            opaque_server_id: response.opaque_server_id,
            initial_hsm_key: response.initial_hsm_key,
        };

        match serde_json::to_vec(&cached) {
            Ok(bytes) => {
                if let Err(e) = self
                    .response_store
                    .put(
                        &state_init_response_key(&request_id),
                        &bytes,
                        self.response_ttl_seconds,
                    )
                    .await
                {
                    error!("Failed to publish state-init response {}: {}", request_id, e);
                } else {
                    info!("Published state-init response for requestId: {}", request_id);
                }
            }
            Err(e) => error!(
                "Failed to serialize CachedStateInitResponse {}: {}",
                request_id, e
            ),
        }
    }
}
