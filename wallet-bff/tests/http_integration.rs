// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use wallet_bff::application::port::outgoing::{
    DeviceStatePort, NoncePort, RequestContextPort, RequestSenderPort, ResponseStorePort,
    StateInitSenderPort,
};
use wallet_bff::application::service::{hsm_response_key, state_init_response_key};
use wallet_bff::domain::{
    CachedResponse, CachedStateInitResponse, Curve, HsmWorkerRequest, OuterResponse,
    RequestContext, StateInitRequest, Status, TypedJws,
};
use wallet_bff::infrastructure::adapters::incoming::web;
use wallet_bff::infrastructure::adapters::incoming::web::handlers::AppState;
use wallet_bff::infrastructure::adapters::incoming::web::replay_protection::ReplayProtectionState;

// ---------------------------------------------------------------------------
// Hand-written test mocks
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

struct MockStateInitSenderPort {
    sent: Arc<Mutex<Vec<StateInitRequest>>>,
}

#[async_trait::async_trait]
impl StateInitSenderPort for MockStateInitSenderPort {
    async fn send(&self, request: &StateInitRequest, _device_id: &str) -> Result<(), String> {
        self.sent.lock().unwrap().push(request.clone());
        Ok(())
    }
}

#[derive(Default)]
struct MockResponseStore {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

#[async_trait::async_trait]
impl ResponseStorePort for MockResponseStore {
    async fn put(&self, key: &str, value: &[u8], _ttl_seconds: u64) -> Result<(), String> {
        self.values.lock().unwrap().insert(key.to_string(), value.to_vec());
        Ok(())
    }
    async fn await_value(
        &self,
        key: &str,
        _timeout_seconds: u64,
    ) -> Result<Option<Vec<u8>>, String> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }
}

#[derive(Default)]
struct MockRequestContextPort {
    contexts: Mutex<HashMap<String, RequestContext>>,
}

#[async_trait::async_trait]
impl RequestContextPort for MockRequestContextPort {
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

struct MockNoncePort;

#[async_trait::async_trait]
impl NoncePort for MockNoncePort {
    async fn try_store(
        &self,
        _client_id: &str,
        _nonce: &str,
        _ttl_seconds: u64,
    ) -> Result<bool, String> {
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Test app factory
// ---------------------------------------------------------------------------

#[derive(Default)]
struct TestAppConfig {
    device_state: Option<String>,
    /// Pre-populated cached HSM response (by request_id) — simulates the case
    /// where the Kafka consumer has already written the envelope.
    preset_hsm_response: Option<(String, CachedResponse)>,
    /// Pre-populated cached state-init response.
    preset_state_init: Option<(String, CachedStateInitResponse)>,
}

struct TestContext {
    app: Router,
    sent_requests: Arc<Mutex<Vec<HsmWorkerRequest>>>,
    sent_state_inits: Arc<Mutex<Vec<StateInitRequest>>>,
    response_store: Arc<MockResponseStore>,
    request_context: Arc<MockRequestContextPort>,
}

fn make_test_app(cfg: TestAppConfig) -> TestContext {
    let sent_requests = Arc::new(Mutex::new(vec![]));
    let sent_state_inits = Arc::new(Mutex::new(vec![]));
    let response_store = Arc::new(MockResponseStore::default());
    let request_context = Arc::new(MockRequestContextPort::default());

    if let Some((request_id, cached)) = cfg.preset_hsm_response {
        let bytes = serde_json::to_vec(&cached).unwrap();
        response_store
            .values
            .lock()
            .unwrap()
            .insert(hsm_response_key(&request_id), bytes);
    }
    if let Some((request_id, cached)) = cfg.preset_state_init {
        let bytes = serde_json::to_vec(&cached).unwrap();
        response_store
            .values
            .lock()
            .unwrap()
            .insert(state_init_response_key(&request_id), bytes);
    }

    let state = Arc::new(AppState {
        device_state_port: Arc::new(MockDeviceStatePort {
            state: cfg.device_state,
        }),
        request_sender_port: Arc::new(MockRequestSenderPort {
            sent: sent_requests.clone(),
        }),
        state_init_sender_port: Arc::new(MockStateInitSenderPort {
            sent: sent_state_inits.clone(),
        }),
        response_store: response_store.clone(),
        request_context_port: request_context.clone(),
        long_poll_timeout_seconds: 0, // fast-path only: don't actually block in unit tests
        response_events_template_url: "http://localhost/hsm/v1/requests/%s".to_string(),
        state_init_events_template_url: "http://localhost/hsm/v1/device-states/%s".to_string(),
        default_initial_key_curve: Curve::P256,
    });

    let rp_state = Arc::new(ReplayProtectionState {
        nonce_port: Arc::new(MockNoncePort),
        nonce_ttl_seconds: 600,
    });

    TestContext {
        app: web::router(state, rp_state),
        sent_requests,
        sent_state_inits,
        response_store,
        request_context,
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

fn ok_cached_response(request_id: &str) -> CachedResponse {
    CachedResponse {
        request_id: request_id.to_string(),
        state_jws: None,
        outer_response_jws: Some(TypedJws::<OuterResponse>::new(
            "some-jws-result".to_string(),
        )),
        status: Status::Ok,
        error_message: None,
    }
}

fn ok_cached_state_init(request_id: &str) -> CachedStateInitResponse {
    CachedStateInitResponse {
        request_id: request_id.to_string(),
        client_id: "assigned-client-id".to_string(),
        state_jws: "fresh-state".to_string(),
        dev_authorization_code: "abc123".to_string(),
        server_jws_public_key: None,
        server_jws_kid: None,
        opaque_server_id: None,
        initial_hsm_key: None,
    }
}

/// Build a minimal fake JWS whose payload is an OuterRequest JSON containing a fresh nonce.
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
// POST /hsm/v1/requests — always 202 + Location, never waits
// ---------------------------------------------------------------------------

#[tokio::test]
async fn post_hsm_request_returns_202_with_location() {
    let ctx = make_test_app(TestAppConfig {
        device_state: Some("mock-state-jws".to_string()),
        ..Default::default()
    });

    let body = serde_json::json!({
        "clientId": "test-client",
        "outerRequestJws": build_test_outer_jws()
    });

    let response = ctx
        .app
        .clone()
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
    let location = response
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header present");
    assert!(
        location.starts_with("http://localhost/hsm/v1/requests/"),
        "Location must point to poll URL, got: {location}"
    );

    let sent = ctx.sent_requests.lock().unwrap();
    assert_eq!(sent.len(), 1, "one request must be sent to Kafka");

    // The handler must persist the request context for the response consumer.
    let ctxs = ctx.request_context.contexts.lock().unwrap();
    assert_eq!(ctxs.len(), 1);
    assert_eq!(
        ctxs.values().next().unwrap().client_id,
        "test-client",
        "context must carry the client_id"
    );
}

#[tokio::test]
async fn post_hsm_request_missing_device_state_returns_404() {
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

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_hsm_request_with_inline_state_jws_skips_lookup() {
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

    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

// ---------------------------------------------------------------------------
// GET /hsm/v1/requests/{correlationId}
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_hsm_request_returns_200_when_cached() {
    let request_id = "550e8400-e29b-41d4-a716-446655440000";
    let ctx = make_test_app(TestAppConfig {
        preset_hsm_response: Some((request_id.to_string(), ok_cached_response(request_id))),
        ..Default::default()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/hsm/v1/requests/{}", request_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let dto = read_body_json(response).await;
    assert_eq!(dto["status"], "complete");
    assert_eq!(dto["result"], "some-jws-result");
}

#[tokio::test]
async fn get_hsm_request_returns_202_when_pending() {
    let ctx = make_test_app(TestAppConfig::default());

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
    assert_eq!(dto["status"], "pending");
    assert!(dto["resultUrl"].as_str().is_some());
}

// ---------------------------------------------------------------------------
// POST /hsm/v1/device-states — always 202 + Location
// ---------------------------------------------------------------------------

#[tokio::test]
async fn post_device_state_returns_202_with_location() {
    let ctx = make_test_app(TestAppConfig::default());

    let body = serde_json::json!({
        "publicKey": dummy_public_key_json(),
        "overwrite": false
    });

    let response = ctx
        .app
        .clone()
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

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header present");
    assert!(
        location.starts_with("http://localhost/hsm/v1/device-states/"),
        "Location must point to state-init poll URL, got: {location}"
    );

    assert_eq!(
        ctx.sent_state_inits.lock().unwrap().len(),
        1,
        "one state-init request must be sent to Kafka"
    );
}

#[tokio::test]
async fn get_device_state_returns_200_when_cached() {
    let request_id = "660e8400-e29b-41d4-a716-446655440000";
    let ctx = make_test_app(TestAppConfig {
        preset_state_init: Some((request_id.to_string(), ok_cached_state_init(request_id))),
        ..Default::default()
    });

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/hsm/v1/device-states/{}", request_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let dto = read_body_json(response).await;
    assert_eq!(dto["clientId"], "assigned-client-id");
    assert_eq!(dto["devAuthorizationCode"], "abc123");
    assert_eq!(dto["stateJws"], "fresh-state");
}

#[tokio::test]
async fn get_device_state_returns_202_when_pending() {
    let ctx = make_test_app(TestAppConfig::default());

    let response = ctx
        .app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/hsm/v1/device-states/660e8400-e29b-41d4-a716-446655440000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let dto = read_body_json(response).await;
    assert_eq!(dto["status"], "pending");
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

// Reference response_store to silence unused warnings on the field.
#[tokio::test]
async fn response_store_field_is_used() {
    let ctx = make_test_app(TestAppConfig::default());
    assert!(ctx.response_store.values.lock().unwrap().is_empty());
}
