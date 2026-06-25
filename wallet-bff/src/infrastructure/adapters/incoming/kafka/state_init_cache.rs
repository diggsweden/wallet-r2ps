// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::oneshot;
use tracing::warn;

use crate::application::port::outgoing::{DeviceStatePort, StateInitCorrelationPort};
use crate::domain::{EcPublicJwk, StateInitResponse};

struct StateInitPending {
    state_key: String,
    ttl_seconds: u64,
    tx: oneshot::Sender<StateInitResponse>,
}

/// Separate from ResponseService intentionally: state initialisation may be
/// performed by a different set of HSMs (and hsm-worker instances) than regular
/// operations, so the two response flows must remain independent.
pub struct StateInitCorrelationService {
    pending: Mutex<HashMap<String, StateInitPending>>,
    device_state_port: Arc<dyn DeviceStatePort>,
    /// Shared with AppState — populated from the first Kafka response that carries a JWE key.
    server_jwe_public_key: Arc<OnceLock<EcPublicJwk>>,
}

impl StateInitCorrelationService {
    pub fn new(
        device_state_port: Arc<dyn DeviceStatePort>,
        server_jwe_public_key: Arc<OnceLock<EcPublicJwk>>,
    ) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            device_state_port,
            server_jwe_public_key,
        }
    }
}

#[async_trait::async_trait]
impl StateInitCorrelationPort for StateInitCorrelationService {
    async fn register_pending(
        &self,
        request_id: &str,
        state_key: &str,
        ttl_seconds: u64,
    ) -> oneshot::Receiver<StateInitResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            request_id.to_string(),
            StateInitPending {
                state_key: state_key.to_string(),
                ttl_seconds,
                tx,
            },
        );
        rx
    }

    async fn response_received(&self, response: StateInitResponse) {
        if let Some(ref key) = response.server_jwe_public_key {
            let _ = self.server_jwe_public_key.set(key.clone());
        }

        let entry = self.pending.lock().unwrap().remove(&response.request_id);

        let Some(e) = entry else {
            warn!(
                "No pending context for state-init requestId: {}, ignoring",
                response.request_id
            );
            return;
        };

        // Save device state before notifying the waiter.
        self.device_state_port
            .save(&e.state_key, &response.state_jws, e.ttl_seconds)
            .await;

        // Ignore send error: HTTP handler may have already timed out.
        let _ = e.tx.send(response);
    }
}
