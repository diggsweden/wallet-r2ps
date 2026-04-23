// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use wallet_bff::application::port::incoming::ResponseUseCase;
use wallet_bff::application::port::outgoing::DeviceStatePort;
use wallet_bff::application::service::ResponseService;
use wallet_bff::domain::{HsmWorkerResponse, OuterResponse, Status, TypedJws};

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

// ── Fixtures ─────────────────────────────────────────────────────────────

fn make_service() -> (Arc<MockDeviceState>, ResponseService) {
    let ds = MockDeviceState::new();
    let svc = ResponseService::new(ds.clone(), Duration::from_secs(60));
    (ds, svc)
}

fn worker_response(state_jws: Option<&str>) -> HsmWorkerResponse {
    HsmWorkerResponse {
        request_id: "req-1".to_string(),
        state_jws: state_jws.map(str::to_string),
        outer_response_jws: Some(TypedJws::<OuterResponse>::new("outer.jws".to_string())),
        status: Status::Ok,
        error_message: None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn response_ready_delivers_to_registered_pending() {
    let (_ds, svc) = make_service();
    let rx = svc.register_pending("req-1", "device-1", 100);
    svc.response_ready(worker_response(None));
    let result = rx.await.expect("channel closed");
    assert_eq!(result.request_id, "req-1");
    assert_eq!(result.status, Status::Ok);
}

#[tokio::test]
async fn response_ready_no_pending_parks_in_cache() {
    let (_ds, svc) = make_service();
    svc.response_ready(worker_response(None));
    let result = svc.wait_for_response("req-1", 100).await;
    assert!(result.is_some());
    assert_eq!(result.unwrap().request_id, "req-1");
}

#[tokio::test]
async fn response_ready_with_state_jws_saves_device_state() {
    let (ds, svc) = make_service();
    let _rx = svc.register_pending("req-1", "device-1", 100);
    svc.response_ready(worker_response(Some("new.state.jws")));
    // Device state is saved via tokio::spawn — give it time to run.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let saves = ds.saves.lock().unwrap();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].0, "device-1");
    assert_eq!(saves[0].1, "new.state.jws");
    assert_eq!(saves[0].2, 100);
}

#[tokio::test]
async fn response_ready_without_state_jws_skips_device_state() {
    let (ds, svc) = make_service();
    let _rx = svc.register_pending("req-1", "device-1", 100);
    svc.response_ready(worker_response(None));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(ds.saves.lock().unwrap().is_empty());
}
