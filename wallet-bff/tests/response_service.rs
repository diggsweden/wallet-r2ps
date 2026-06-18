// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use wallet_bff::application::port::incoming::{HsmResponseSinkPort, StateInitResponseSinkPort};
use wallet_bff::application::port::outgoing::{
    DeviceStatePort, RequestContextPort, ResponseStorePort,
};
use wallet_bff::application::service::{
    HsmResponseSinkService, StateInitResponseSinkService, hsm_response_key,
    state_init_response_key,
};
use wallet_bff::domain::{
    CachedResponse, CachedStateInitResponse, HsmWorkerResponse, OuterResponse, RequestContext,
    StateInitResponse, Status, TypedJws,
};

// ── Mocks ────────────────────────────────────────────────────────────────

struct MockDeviceState {
    saves: Mutex<Vec<(String, String, u64)>>,
}

impl MockDeviceState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            saves: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl DeviceStatePort for MockDeviceState {
    async fn save(&self, key: &str, state: &str, ttl_seconds: u64) {
        self.saves
            .lock()
            .unwrap()
            .push((key.to_string(), state.to_string(), ttl_seconds));
    }
    async fn load(&self, _key: &str) -> Option<String> {
        None
    }
}

#[derive(Default)]
struct MockResponseStore {
    puts: Mutex<HashMap<String, Vec<u8>>>,
}

impl MockResponseStore {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait]
impl ResponseStorePort for MockResponseStore {
    async fn put(&self, key: &str, value: &[u8], _ttl_seconds: u64) -> Result<(), String> {
        self.puts.lock().unwrap().insert(key.to_string(), value.to_vec());
        Ok(())
    }
    async fn await_value(
        &self,
        key: &str,
        _timeout_seconds: u64,
    ) -> Result<Option<Vec<u8>>, String> {
        Ok(self.puts.lock().unwrap().get(key).cloned())
    }
}

struct MockRequestContext {
    contexts: Mutex<HashMap<String, RequestContext>>,
}

impl MockRequestContext {
    fn with(request_id: &str, client_id: &str, ttl: u64) -> Arc<Self> {
        let mut m = HashMap::new();
        m.insert(
            request_id.to_string(),
            RequestContext {
                client_id: client_id.to_string(),
                ttl_seconds: ttl,
            },
        );
        Arc::new(Self {
            contexts: Mutex::new(m),
        })
    }
    fn empty() -> Arc<Self> {
        Arc::new(Self {
            contexts: Mutex::new(HashMap::new()),
        })
    }
}

#[async_trait]
impl RequestContextPort for MockRequestContext {
    async fn store(
        &self,
        request_id: &str,
        ctx: &RequestContext,
        _ttl_seconds: u64,
    ) -> Result<(), String> {
        self.contexts
            .lock()
            .unwrap()
            .insert(request_id.to_string(), ctx.clone());
        Ok(())
    }
    async fn take(&self, request_id: &str) -> Option<RequestContext> {
        self.contexts.lock().unwrap().remove(request_id)
    }
}

// ── Fixtures ─────────────────────────────────────────────────────────────

fn worker_response(state_jws: Option<&str>) -> HsmWorkerResponse {
    HsmWorkerResponse {
        request_id: "req-1".to_string(),
        state_jws: state_jws.map(str::to_string),
        outer_response_jws: Some(TypedJws::<OuterResponse>::new("outer.jws".to_string())),
        status: Status::Ok,
        error_message: None,
    }
}

fn state_init_response(request_id: &str, state_jws: &str) -> StateInitResponse {
    StateInitResponse {
        request_id: request_id.to_string(),
        state_jws: state_jws.to_string(),
        dev_authorization_code: "auth-code".to_string(),
        server_jws_public_key: None,
        server_jws_kid: None,
        opaque_server_id: None,
        initial_hsm_key: None,
    }
}

// ── HsmResponseSinkService ────────────────────────────────────────────────

#[tokio::test]
async fn hsm_sink_publishes_envelope_to_store() {
    let ds = MockDeviceState::new();
    let store = MockResponseStore::new();
    let ctx = MockRequestContext::with("req-1", "device-1", 100);
    let sink = HsmResponseSinkService::new(ds.clone(), store.clone(), ctx, 60);

    sink.ingest(worker_response(None)).await;
    // Allow spawn(or direct) to complete (no spawn here, but be safe).
    tokio::time::sleep(Duration::from_millis(10)).await;

    let stored = store
        .puts
        .lock()
        .unwrap()
        .get(&hsm_response_key("req-1"))
        .cloned()
        .expect("envelope stored");
    let cached: CachedResponse = serde_json::from_slice(&stored).unwrap();
    assert_eq!(cached.request_id, "req-1");
    assert_eq!(cached.status, Status::Ok);
}

#[tokio::test]
async fn hsm_sink_saves_state_when_present() {
    let ds = MockDeviceState::new();
    let store = MockResponseStore::new();
    let ctx = MockRequestContext::with("req-1", "device-1", 100);
    let sink = HsmResponseSinkService::new(ds.clone(), store, ctx, 60);

    sink.ingest(worker_response(Some("new.state.jws"))).await;

    let saves = ds.saves.lock().unwrap();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].0, "device-1");
    assert_eq!(saves[0].1, "new.state.jws");
    assert_eq!(saves[0].2, 100);
}

#[tokio::test]
async fn hsm_sink_without_state_skips_device_state_save() {
    let ds = MockDeviceState::new();
    let store = MockResponseStore::new();
    let ctx = MockRequestContext::with("req-1", "device-1", 100);
    let sink = HsmResponseSinkService::new(ds.clone(), store, ctx, 60);

    sink.ingest(worker_response(None)).await;

    assert!(ds.saves.lock().unwrap().is_empty());
}

#[tokio::test]
async fn hsm_sink_publishes_envelope_even_without_context() {
    // Stale duplicate Kafka delivery: context was already consumed. The sink
    // must still publish the response so any waiter can see it.
    let ds = MockDeviceState::new();
    let store = MockResponseStore::new();
    let ctx = MockRequestContext::empty();
    let sink = HsmResponseSinkService::new(ds.clone(), store.clone(), ctx, 60);

    sink.ingest(worker_response(Some("new.state.jws"))).await;

    assert!(
        store
            .puts
            .lock()
            .unwrap()
            .contains_key(&hsm_response_key("req-1"))
    );
    // No context means we cannot attribute the state to a device; skip the save.
    assert!(ds.saves.lock().unwrap().is_empty());
}

// ── StateInitResponseSinkService ─────────────────────────────────────────

#[tokio::test]
async fn state_init_sink_publishes_envelope_with_client_id() {
    let ds = MockDeviceState::new();
    let store = MockResponseStore::new();
    let ctx = MockRequestContext::with("init-1", "client-42", 600);
    let sink = StateInitResponseSinkService::new(ds.clone(), store.clone(), ctx, 60);

    sink.ingest(state_init_response("init-1", "fresh.state.jws"))
        .await;

    let stored = store
        .puts
        .lock()
        .unwrap()
        .get(&state_init_response_key("init-1"))
        .cloned()
        .expect("envelope stored");
    let cached: CachedStateInitResponse = serde_json::from_slice(&stored).unwrap();
    assert_eq!(cached.client_id, "client-42");
    assert_eq!(cached.state_jws, "fresh.state.jws");
    assert_eq!(cached.dev_authorization_code, "auth-code");

    let saves = ds.saves.lock().unwrap();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].0, "client-42");
    assert_eq!(saves[0].1, "fresh.state.jws");
    assert_eq!(saves[0].2, 600);
}

#[tokio::test]
async fn state_init_sink_drops_response_without_context() {
    let ds = MockDeviceState::new();
    let store = MockResponseStore::new();
    let ctx = MockRequestContext::empty();
    let sink = StateInitResponseSinkService::new(ds.clone(), store.clone(), ctx, 60);

    sink.ingest(state_init_response("init-1", "fresh.state.jws"))
        .await;

    assert!(store.puts.lock().unwrap().is_empty());
    assert!(ds.saves.lock().unwrap().is_empty());
}
