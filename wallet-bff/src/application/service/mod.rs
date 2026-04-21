// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::application::port::incoming::ResponseUseCase;
use crate::application::port::outgoing::DeviceStatePort;
use crate::domain::{CachedResponse, HsmWorkerResponse};
use std::sync::Arc;

struct PendingEntry {
    state_key: String,
    ttl_seconds: u64,
    tx: oneshot::Sender<CachedResponse>,
}

struct CachedEntry {
    response: CachedResponse,
    expires_at: Instant,
}

pub struct ResponseService {
    device_state_port: Arc<dyn DeviceStatePort>,
    pending: Mutex<HashMap<String, PendingEntry>>,
    cache: Mutex<HashMap<String, CachedEntry>>,
    response_ttl: Duration,
}

impl ResponseService {
    pub fn new(device_state_port: Arc<dyn DeviceStatePort>, response_ttl: Duration) -> Self {
        Self {
            device_state_port,
            pending: Mutex::new(HashMap::new()),
            cache: Mutex::new(HashMap::new()),
            response_ttl,
        }
    }

    fn store_in_cache(&self, response: CachedResponse) {
        let entry = CachedEntry {
            expires_at: Instant::now() + self.response_ttl,
            response,
        };
        self.cache
            .lock()
            .unwrap()
            .insert(entry.response.request_id.clone(), entry);
    }
}

#[async_trait::async_trait]
impl ResponseUseCase for ResponseService {
    fn register_pending(
        &self,
        request_id: &str,
        state_key: &str,
        ttl_seconds: u64,
    ) -> oneshot::Receiver<CachedResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            request_id.to_string(),
            PendingEntry {
                state_key: state_key.to_string(),
                ttl_seconds,
                tx,
            },
        );
        rx
    }

    fn response_ready(&self, response: HsmWorkerResponse) {
        let entry = self.pending.lock().unwrap().remove(&response.request_id);

        let cached = CachedResponse::from(response);

        if let Some(e) = entry {
            // Save device state asynchronously via a spawned task so we don't
            // block the Kafka consumer thread. The state_key and ttl come from
            // the in-memory pending entry rather than Redis.
            if let Some(ref state_jws) = cached.state_jws {
                let port = self.device_state_port.clone();
                let key = e.state_key.clone();
                let jws = state_jws.clone();
                let ttl = e.ttl_seconds;
                tokio::spawn(async move {
                    port.save(&key, &jws, ttl).await;
                });
            }

            info!("Response ready for requestId: {}", cached.request_id);

            if e.tx.send(cached.clone()).is_err() {
                // Receiver was dropped (HTTP handler timed out); park in cache.
                self.store_in_cache(cached);
            }
        } else {
            // No sync waiter — this can happen when a pod restarts and reuses a
            // topic that still has responses from the previous session.
            // TODO: forward undeliverable responses to a shared durable store
            // (e.g. a dedicated Kafka topic) so clients can reconnect and
            // retrieve the result of a request that outlived its original pod.
            warn!(
                "No pending entry for requestId: {}, caching for polling",
                cached.request_id
            );
            self.store_in_cache(cached);
        }
    }

    async fn wait_for_response(&self, request_id: &str, timeout_ms: u64) -> Option<CachedResponse> {
        // TODO: cache is in-memory — async GET polling requires sticky sessions in the load
        // balancer, otherwise a poll landing on a different instance than the original POST
        // will miss the response.
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            {
                let mut map = self.cache.lock().unwrap();
                if let Some(entry) = map.get(request_id) {
                    if entry.expires_at > Instant::now() {
                        return Some(entry.response.clone());
                    }
                    map.remove(request_id);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
