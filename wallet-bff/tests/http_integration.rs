// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tower::ServiceExt;

use wallet_bff::application::port::incoming::ResponseUseCase;
use wallet_bff::application::port::outgoing::{
    DeviceStatePort, NoncePort, RequestSenderPort, StateInitCorrelationPort, StateInitSenderPort,
};
use wallet_bff::domain::{
    CachedResponse, Curve, HsmWorkerRequest, HsmWorkerResponse, OuterResponse, StateInitRequest,
    StateInitResponse, Status, TypedJws,
};
use wallet_bff::infrastructure::adapters::incoming::web;
use wallet_bff::infrastructure::adapters::incoming::web::handlers::AppState;
use wallet_bff::infrastructure::adapters::incoming::web::replay_protection::ReplayProtectionState;

// ---------------------------------------------------------------------------
// Hand-written test mocks (consistent with existing response_service test style)
// ---------------------------------------------------------------------------

struct MockDeviceStatePort {
    state: Option<String>,
}

#[async_trait::async_trait]
impl DeviceStatePort for MockDeviceStatePort {
    async fn save(&self, _key: &str, _state: &str, _ttl_seconds: u64) {}
    async fn load(&self, _key: &str) -> Option<String> {
        self.state.clone()
    }
}

struct MockRequestSenderPort {
    sent: Arc<Mutex<Vec<HsmWorkerRequest>>>,
}

#[async_trait::async_trait]
impl RequestSenderPort for MockRequestSenderPort {
    async fn send(&self, request: &HsmWorkerRequest, _device_id: &str) -> Result<(), String> {
        self.sent.lock().unwrap().push(request.clone());
        Ok(())
    }
}

struct MockStateInitSenderPort;

#[async_trait::async_trait]
impl StateInitSenderPort for MockStateInitSenderPort {
    async fn send(&self, _request: &StateInitRequest, _device_id: &str) -> Result<(), String> {
        Ok(())
    }
}

struct MockResponseUseCase {
    sync_response: Option<CachedResponse>,
}

#[async_trait::async_trait]
impl ResponseUseCase for MockResponseUseCase {
    fn register_pending(
        &self,
        _request_id: &str,
        _state_key: &str,
        _ttl_seconds: u64,
    ) -> oneshot::Receiver<CachedResponse> {
        let (tx, rx) = oneshot::channel();
        if let Some(ref r) = self.sync_response {
            let _ = tx.send(r.clone());
        }
        // If sync_response is None, tx is dropped and rx returns Err on await.
        rx
    }

    fn response_ready(&self, _response: HsmWorkerResponse) {}

    async fn wait_for_response(
        &self,
        _request_id: &str,
        _timeout_ms: u64,
    ) -> Option<CachedResponse> {
        self.sync_response.clone()
    }
}

struct MockNoncePort;

#[async_trait::async_trait]
impl NoncePort for MockNoncePort {
    async fn try_store(
        &self,
        _client_id: &str,
        _nonce: &str,
        _ttl_seconds: u64,
    ) -> Result<bool, String> {
        Ok(true) // always accept in tests
    }
}

struct MockStateInitCorrelationPort {
    response: Option<StateInitResponse>,
}

#[async_trait::async_trait]
impl StateInitCorrelationPort for MockStateInitCorrelationPort {
    async fn register_pending(
        &self,
        _request_id: &str,
        _state_key: &str,
        _ttl_seconds: u64,
    ) -> oneshot::Receiver<StateInitResponse> {
        let (tx, rx) = oneshot::channel();
        if let Some(ref r) = self.response {
            let _ = tx.send(r.clone());
        }
        rx
    }

    async fn response_received(&self, _response: StateInitResponse) {}
}

// ---------------------------------------------------------------------------
// Test app factory
// ---------------------------------------------------------------------------

struct TestAppConfig {
    device_state: Option<String>,
    sync_response: Option<CachedResponse>,
    state_init_response: Option<StateInitResponse>,
    serve_sync: bool,
    sync_timeout_ms: u64,
    state_init_timeout_ms: u64,
}

impl Default for TestAppConfig {
    fn default() -> Self {
        Self {
            device_state: Some("mock-state-jws".to_string()),
            sync_response: None,
            state_init_response: None,
            serve_sync: false,
            sync_timeout_ms: 100,
            state_init_timeout_ms: 100,
        }
    }
}

struct TestContext {
    app: Router,
    sent_requests: Arc<Mutex<Vec<HsmWorkerRequest>>>,
}

fn make_test_app(cfg: TestAppConfig) -> TestContext {
    let sent_requests = Arc::new(Mutex::new(vec![]));
    let state = Arc::new(AppState {
        device_state_port: Arc::new(MockDeviceStatePort {
            state: cfg.device_state,
        }),
        request_sender_port: Arc::new(MockRequestSenderPort {
            sent: sent_requests.clone(),
        }),
        state_init_sender_port: Arc::new(MockStateInitSenderPort),
        response_use_case: Arc::new(MockResponseUseCase {
            sync_response: cfg.sync_response,
        }),
        state_init_correlation: Arc::new(MockStateInitCorrelationPort {
            response: cfg.state_init_response,
        }),
        server_jwe_public_key: Arc::new(std::sync::OnceLock::new()),
        serve_sync: cfg.serve_sync,
        sync_timeout_ms: cfg.sync_timeout_ms,
        state_init_timeout_ms: cfg.state_init_timeout_ms,
        response_events_template_url: "http://localhost/hsm/v1/requests/%s".to_string(),
        default_initial_key_curve: Curve::P256,
    });

    let rp_state = Arc::new(ReplayProtectionState {
        nonce_port: Arc::new(MockNoncePort),
        nonce_ttl_seconds: 600,
    });

    TestContext {
        app: web::router(state, rp_state),
        sent_requests,
    }
}

fn dummy_public_key_json() -> serde_json::Value {
    serde_json::json!({
        "kty": "EC",
        "crv": "P-256",
        "x": "x_coord_base64url",
        "y": "y_coord_base64url"
    })
}

fn ok_cached_response() -> CachedResponse {
    CachedResponse {
        request_id: "any-id".to_string(),
        state_jws: None,
        outer_response_jws: Some(TypedJws::<OuterResponse>::new(
            "some-jws-result".to_string(),
        )),
        status: Status::Ok,
        error_message: None,
    }
}

fn ok_state_init_response() -> StateInitResponse {
    StateInitResponse {
        request_id: "any-id".to_string(),
        state_jws: "mock-state-jws".to_string(),
        dev_authorization_code: "abc123".to_string(),
        server_jws_public_key: None,
        server_jwe_public_key: None,
        opaque_server_id: None,
        initial_hsm_key: None,
    }
}

/// Build a minimal fake JWS whose payload is an OuterRequest JSON containing a fresh nonce.
/// The middleware decodes the payload without verifying the signature, so the signature
/// bytes can be anything — tests only need a structurally valid compact JWS.
fn build_test_outer_jws() -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","kid":"test-key"}"#);
    let payload_json = serde_json::json!({
        "version": 1,
        "nonce": "some_nonce"
    });
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload_json).unwrap());
    let sig = URL_SAFE_NO_PAD.encode(b"fakesig");
    format!("{}.{}.{}", header, payload, sig)
}

async fn read_body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// ---------------------------------------------------------------------------
// POST /hsm/v1/requests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn submitting_an_hsm_request_is_accepted_and_forwarded_for_processing() {
    let ctx = make_test_app(TestAppConfig::default());

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": build_test_outer_jws()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/requests")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let sent = ctx.sent_requests.lock().unwrap();
    assert_eq!(sent.len(), 1, "one request must be sent to Kafka");
}

#[tokio::test]
async fn an_hsm_request_with_an_immediately_available_result_is_returned_as_complete() {
    let ctx = make_test_app(TestAppConfig {
        serve_sync: true,
        sync_response: Some(ok_cached_response()),
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": build_test_outer_jws()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/requests")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let dto = read_body_json(response).await;
    assert_eq!(dto["status"], "complete", "status must be 'complete'");
    assert!(!dto["result"].is_null(), "result must be present");
}

#[tokio::test]
async fn an_hsm_request_with_no_result_yet_is_returned_as_pending() {
    let ctx = make_test_app(TestAppConfig {
        serve_sync: true,
        sync_response: None,
        sync_timeout_ms: 1, // very short to avoid test slowdown
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": build_test_outer_jws()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/requests")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let dto = read_body_json(response).await;
    assert_eq!(
        dto["status"], "pending",
        "status must be 'pending' when no response is ready"
    );
}

#[tokio::test]
async fn an_hsm_request_for_an_unknown_device_is_rejected_as_not_found() {
    let ctx = make_test_app(TestAppConfig {
        device_state: None,
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "unknown-client",
        "outerRequestJws": build_test_outer_jws()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/requests")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "missing device state must return 404"
    );
}

// ---------------------------------------------------------------------------
// GET /hsm/v1/requests/{correlationId}
// ---------------------------------------------------------------------------

#[tokio::test]
async fn polling_a_request_with_a_ready_result_returns_it_as_complete() {
    let ctx = make_test_app(TestAppConfig {
        sync_response: Some(ok_cached_response()),
        ..Default::default()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hsm/v1/requests/550e8400-e29b-41d4-a716-446655440000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let dto = read_body_json(response).await;
    assert_eq!(
        dto["status"], "complete",
        "status must be 'complete' when response is ready"
    );
}

#[tokio::test]
async fn polling_a_request_with_no_result_yet_returns_it_as_pending() {
    let ctx = make_test_app(TestAppConfig {
        sync_response: None,
        sync_timeout_ms: 1,
        ..Default::default()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hsm/v1/requests/550e8400-e29b-41d4-a716-446655440000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let dto = read_body_json(response).await;
    assert_eq!(
        dto["status"], "pending",
        "status must be 'pending' when no response is cached"
    );
}

// ---------------------------------------------------------------------------
// POST /hsm/v1/device-states
// ---------------------------------------------------------------------------

#[tokio::test]
async fn initializing_a_new_device_state_returns_a_device_authorization_code() {
    let ctx = make_test_app(TestAppConfig {
        device_state: None, // no pre-existing state so handler doesn't short-circuit
        state_init_response: Some(ok_state_init_response()),
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientJwsPublicKey": dummy_public_key_json(),
        "clientJwePublicKey": dummy_public_key_json(),
        "overwrite": false
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/device-states")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let dto = read_body_json(response).await;
    assert_eq!(
        dto["devAuthorizationCode"], "abc123",
        "devAuthorizationCode must be present in response"
    );
}

#[tokio::test]
async fn a_device_state_init_request_that_times_out_returns_a_server_error() {
    let ctx = make_test_app(TestAppConfig {
        device_state: None, // no pre-existing state so handler doesn't short-circuit
        state_init_response: None,
        state_init_timeout_ms: 1, // immediate timeout
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientJwsPublicKey": dummy_public_key_json(),
        "clientJwePublicKey": dummy_public_key_json(),
        "overwrite": false
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/device-states")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "timeout must return 500"
    );
}

// ---------------------------------------------------------------------------
// state_jws passthrough tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_hsm_request_carrying_its_own_state_token_succeeds_even_if_no_stored_state_exists() {
    // No state in port — request should succeed because stateJws is in the body.
    let ctx = make_test_app(TestAppConfig {
        device_state: None,
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": build_test_outer_jws(),
        "stateJws": "inline-state-token"
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/requests")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::ACCEPTED,
        "stateJws in body must prevent 404 even when port has no state"
    );
}

#[tokio::test]
async fn initializing_an_existing_device_state_without_overwrite_returns_the_current_state_token() {
    // Default config has device_state = Some("mock-state-jws").
    let ctx = make_test_app(TestAppConfig::default());

    let body = serde_json::json!({
        "clientJwsPublicKey": dummy_public_key_json(),
        "clientJwePublicKey": dummy_public_key_json(),
        "overwrite": false
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/device-states")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let dto = read_body_json(response).await;
    assert_eq!(
        dto["stateJws"], "mock-state-jws",
        "existing stateJws must be returned when overwrite=false"
    );
}

#[tokio::test]
async fn initializing_a_new_device_state_returns_the_state_token_from_the_worker() {
    let ctx = make_test_app(TestAppConfig {
        device_state: None,
        state_init_response: Some(ok_state_init_response()),
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientJwsPublicKey": dummy_public_key_json(),
        "clientJwePublicKey": dummy_public_key_json(),
        "overwrite": false
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/device-states")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let dto = read_body_json(response).await;
    assert_eq!(
        dto["stateJws"], "mock-state-jws",
        "stateJws from StateInitResponse must be present in success response"
    );
}

#[tokio::test]
async fn a_completed_hsm_request_forwards_the_updated_state_token_from_the_worker() {
    let mut cached = ok_cached_response();
    cached.state_jws = Some("updated-state-token".to_string());

    let ctx = make_test_app(TestAppConfig {
        serve_sync: true,
        sync_response: Some(cached),
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": build_test_outer_jws()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/requests")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let dto = read_body_json(response).await;
    assert_eq!(
        dto["stateJws"], "updated-state-token",
        "stateJws from worker response must be forwarded in complete async response"
    );
}

// ---------------------------------------------------------------------------
// Contract tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn malformed_json_in_a_request_returns_a_problem_json_error() {
    let ctx = make_test_app(TestAppConfig::default());

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/requests")
                .header("content-type", "application/json")
                .body(Body::from("not valid json {{{"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("application/problem+json"),
        "malformed JSON must return application/problem+json, got: {content_type}"
    );
}
