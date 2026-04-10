// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use wallet_bff::application::port::incoming::ResponseUseCase;
use wallet_bff::application::port::outgoing::{
    DeviceStatePort, PendingContextPort, ResponseSinkPort,
};
use wallet_bff::application::service::ResponseService;
use wallet_bff::domain::{CachedResponse, HsmWorkerResponse, PendingRequestContext, Status};

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

struct MockPendingContext {
    response: Option<PendingRequestContext>,
}

impl MockPendingContext {
    fn returning(ctx: Option<PendingRequestContext>) -> Arc<Self> {
        Arc::new(Self { response: ctx })
    }
}

#[async_trait]
impl PendingContextPort for MockPendingContext {
    async fn save(&self, _id: &str, _ctx: &PendingRequestContext) {}
    async fn load(&self, _id: &str) -> Option<PendingRequestContext> {
        self.response.clone()
    }
}

struct MockResponseSink {
    stored: Mutex<Vec<CachedResponse>>,
}

impl MockResponseSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            stored: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl ResponseSinkPort for MockResponseSink {
    async fn store(&self, response: &CachedResponse) {
        self.stored.lock().unwrap().push(response.clone());
    }
    async fn load(&self, _id: &str) -> Option<CachedResponse> {
        None
    }
}

// ── Fixtures ─────────────────────────────────────────────────────────────

struct Mocks {
    device_state: Arc<MockDeviceState>,
    response_sink: Arc<MockResponseSink>,
    service: ResponseService,
}

fn make_service(ctx: Option<PendingRequestContext>) -> Mocks {
    let device_state = MockDeviceState::new();
    let response_sink = MockResponseSink::new();
    let service = ResponseService::new(
        device_state.clone(),
        MockPendingContext::returning(ctx),
        response_sink.clone(),
    );
    Mocks {
        device_state,
        response_sink,
        service,
    }
}

fn worker_response(state_jws: Option<&str>) -> HsmWorkerResponse {
    HsmWorkerResponse {
        request_id: "req-1".to_string(),
        state_jws: state_jws.map(str::to_string),
        outer_response_jws: Some("outer.jws".to_string()),
        status: Status::Ok,
        error_message: None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn response_ready_no_context_persists_nothing() {
    let mocks = make_service(None);
    mocks
        .service
        .response_ready(worker_response(Some("new.state")))
        .await;

    assert!(mocks.device_state.saves.lock().unwrap().is_empty());
    assert!(mocks.response_sink.stored.lock().unwrap().is_empty());
}

#[tokio::test]
async fn response_ready_with_state_jws_saves_device_state() {
    let ctx = PendingRequestContext {
        state_key: "device-1".to_string(),
        ttl_seconds: 100,
    };
    let mocks = make_service(Some(ctx));
    mocks
        .service
        .response_ready(worker_response(Some("new.state.jws")))
        .await;

    let saves = mocks.device_state.saves.lock().unwrap();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].0, "device-1");
    assert_eq!(saves[0].1, "new.state.jws");
    assert_eq!(saves[0].2, 100);

    assert_eq!(mocks.response_sink.stored.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn response_ready_without_state_jws_skips_device_state() {
    let ctx = PendingRequestContext {
        state_key: "device-1".to_string(),
        ttl_seconds: 100,
    };
    let mocks = make_service(Some(ctx));
    mocks.service.response_ready(worker_response(None)).await;

    assert!(mocks.device_state.saves.lock().unwrap().is_empty());
    assert_eq!(mocks.response_sink.stored.lock().unwrap().len(), 1);
}
