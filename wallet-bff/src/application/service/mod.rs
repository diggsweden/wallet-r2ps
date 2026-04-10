// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::application::port::incoming::ResponseUseCase;
use crate::application::port::outgoing::{DeviceStatePort, PendingContextPort, ResponseSinkPort};
use crate::domain::{CachedResponse, HsmWorkerResponse};

pub struct ResponseService {
    device_state_port: Arc<dyn DeviceStatePort>,
    pending_context_port: Arc<dyn PendingContextPort>,
    response_sink_port: Arc<dyn ResponseSinkPort>,
}

impl ResponseService {
    pub fn new(
        device_state_port: Arc<dyn DeviceStatePort>,
        pending_context_port: Arc<dyn PendingContextPort>,
        response_sink_port: Arc<dyn ResponseSinkPort>,
    ) -> Self {
        Self {
            device_state_port,
            pending_context_port,
            response_sink_port,
        }
    }
}

#[async_trait::async_trait]
impl ResponseUseCase for ResponseService {
    async fn response_ready(&self, response: HsmWorkerResponse) {
        let ctx = self.pending_context_port.load(&response.request_id).await;

        let Some(ctx) = ctx else {
            warn!(
                "No pending context for requestId: {}, ignoring",
                response.request_id
            );
            return;
        };

        if let Some(ref state_jws) = response.state_jws {
            self.device_state_port
                .save(&ctx.state_key, state_jws, ctx.ttl_seconds)
                .await;
        }

        let cached = CachedResponse::from(response);
        self.response_sink_port.store(&cached).await;
        info!("Stored response for requestId: {}", cached.request_id);
    }

    async fn wait_for_response(&self, request_id: &str, timeout_ms: u64) -> Option<CachedResponse> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            if let Some(resp) = self.response_sink_port.load(request_id).await {
                info!("Got response for requestId: {}", request_id);
                return Some(resp);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
