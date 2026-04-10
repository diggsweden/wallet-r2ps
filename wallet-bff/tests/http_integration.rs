// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

use wallet_bff::application::port::incoming::ResponseUseCase;
use wallet_bff::application::port::outgoing::{
    DeviceStatePort, PendingContextPort, RequestSenderPort, StateInitCachePort, StateInitSenderPort,
};
use wallet_bff::domain::{
    CachedResponse, HsmWorkerRequest, HsmWorkerResponse, OuterResponse, PendingRequestContext,
    StateInitRequest, StateInitResponse, Status, TypedJws,
};
use wallet_bff::infrastructure::adapters::incoming::web;
use wallet_bff::infrastructure::adapters::incoming::web::handlers::AppState;

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

struct MockPendingContextPort;

#[async_trait::async_trait]
impl PendingContextPort for MockPendingContextPort {
    async fn save(&self, _request_id: &str, _ctx: &PendingRequestContext) {}
    async fn load(&self, _request_id: &str) -> Option<PendingRequestContext> {
        None
    }
}

struct MockResponseUseCase {
    sync_response: Option<CachedResponse>,
}

#[async_trait::async_trait]
impl ResponseUseCase for MockResponseUseCase {
    async fn response_ready(&self, _response: HsmWorkerResponse) {}
    async fn wait_for_response(
        &self,
        _request_id: &str,
        _timeout_ms: u64,
    ) -> Option<CachedResponse> {
        self.sync_response.clone()
    }
}

struct MockStateInitCachePort {
    response: Option<StateInitResponse>,
}

#[async_trait::async_trait]
impl StateInitCachePort for MockStateInitCachePort {
    async fn wait_for_response(
        &self,
        _request_id: &str,
        _timeout: Duration,
    ) -> Option<StateInitResponse> {
        self.response.clone()
    }

    async fn put(&self, _request_id: String, _response: StateInitResponse) {}
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
        pending_context_port: Arc::new(MockPendingContextPort),
        response_use_case: Arc::new(MockResponseUseCase {
            sync_response: cfg.sync_response,
        }),
        state_init_cache: Arc::new(MockStateInitCachePort {
            response: cfg.state_init_response,
        }),
        serve_sync: cfg.serve_sync,
        sync_timeout_ms: cfg.sync_timeout_ms,
        state_init_timeout_ms: cfg.state_init_timeout_ms,
        response_events_template_url: "http://localhost/hsm/v1/requests/%s".to_string(),
    });

    TestContext {
        app: web::router(state),
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
        server_jws_kid: None,
        opaque_server_id: None,
    }
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
async fn test_post_hsm_request_sends_to_kafka() {
    let ctx = make_test_app(TestAppConfig::default());

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": "mock-outer-jws"
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
async fn test_post_hsm_request_sync_returns_result() {
    let ctx = make_test_app(TestAppConfig {
        serve_sync: true,
        sync_response: Some(ok_cached_response()),
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": "mock-outer-jws"
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
async fn test_post_hsm_request_sync_timeout_returns_pending() {
    let ctx = make_test_app(TestAppConfig {
        serve_sync: true,
        sync_response: None,
        sync_timeout_ms: 1, // very short to avoid test slowdown
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": "mock-outer-jws"
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
async fn test_post_hsm_request_missing_device_state() {
    let ctx = make_test_app(TestAppConfig {
        device_state: None,
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "unknown-client",
        "outerRequestJws": "mock-outer-jws"
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
async fn test_get_hsm_request_complete() {
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
async fn test_get_hsm_request_pending() {
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
async fn test_post_device_state_init_success() {
    let ctx = make_test_app(TestAppConfig {
        device_state: None, // no pre-existing state so handler doesn't short-circuit
        state_init_response: Some(ok_state_init_response()),
        ..Default::default()
    });

    let body = serde_json::json!({
        "publicKey": dummy_public_key_json(),
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
async fn test_post_device_state_init_timeout() {
    let ctx = make_test_app(TestAppConfig {
        device_state: None, // no pre-existing state so handler doesn't short-circuit
        state_init_response: None,
        state_init_timeout_ms: 1, // immediate timeout
        ..Default::default()
    });

    let body = serde_json::json!({
        "publicKey": dummy_public_key_json(),
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
// POST /hsm/v1/operations (legacy synchronous endpoint)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_post_legacy_operations_with_result_returns_jws_string() {
    let ctx = make_test_app(TestAppConfig {
        serve_sync: true,
        sync_response: Some(ok_cached_response()),
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": "mock-outer-jws"
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/operations")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    // Legacy endpoint returns the raw JWS result string (not JSON envelope)
    let result_str = std::str::from_utf8(&bytes).unwrap();
    assert_eq!(result_str, "some-jws-result");
}

#[tokio::test]
async fn test_post_legacy_operations_timeout_returns_408() {
    let ctx = make_test_app(TestAppConfig {
        serve_sync: true,
        sync_response: None,
        sync_timeout_ms: 1,
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": "mock-outer-jws"
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/hsm/v1/operations")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
}

// ---------------------------------------------------------------------------
// Contract tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_malformed_json_returns_problem_json() {
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
