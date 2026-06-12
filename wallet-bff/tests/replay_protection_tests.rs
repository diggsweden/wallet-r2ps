// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::{Router, middleware};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use wallet_bff::application::port::outgoing::NoncePort;
use wallet_bff::infrastructure::adapters::incoming::web::replay_protection::{
    ReplayProtectionState, replay_protection,
};

// ---------------------------------------------------------------------------
// Configurable mock
// ---------------------------------------------------------------------------

struct ConfigurableMockNoncePort {
    result: Arc<Mutex<Result<bool, String>>>,
}

#[async_trait::async_trait]
impl NoncePort for ConfigurableMockNoncePort {
    async fn try_store(
        &self,
        _client_id: &str,
        _nonce: &str,
        _ttl_seconds: u64,
    ) -> Result<bool, String> {
        self.result.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_app(nonce_result: Result<bool, String>) -> Router {
    let rp_state = Arc::new(ReplayProtectionState {
        nonce_port: Arc::new(ConfigurableMockNoncePort {
            result: Arc::new(Mutex::new(nonce_result)),
        }),
        nonce_ttl_seconds: 600,
    });
    Router::new()
        .route(
            "/test",
            get(|| async { StatusCode::OK }).post(|| async { StatusCode::OK }),
        )
        .route("/hsm/v1/device-states", post(|| async { StatusCode::OK }))
        .layer(middleware::from_fn_with_state(rp_state, replay_protection))
}

fn jws_with_nonce(nonce: &str) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","kid":"test-key"}"#);
    let payload = serde_json::json!({ "version": 1, "nonce": nonce });
    let payload_enc = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap());
    let sig = URL_SAFE_NO_PAD.encode(b"fakesig");
    format!("{}.{}.{}", header, payload_enc, sig)
}

fn post_body(client_id: &str, nonce: &str) -> String {
    serde_json::json!({
        "clientId": client_id,
        "outerRequestJws": jws_with_nonce(nonce),
    })
    .to_string()
}

async fn post_to(app: Router, uri: &str, body: String) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_nonce_passes_through() {
    let app = make_app(Ok(true));
    let resp = post_to(app, "/test", post_body("client-a", "nonce-abc")).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn duplicate_nonce_returns_409() {
    let app = make_app(Ok(false));
    let resp = post_to(app, "/test", post_body("client-a", "nonce-abc")).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn nonce_store_error_returns_500() {
    let app = make_app(Err("redis unavailable".to_string()));
    let resp = post_to(app, "/test", post_body("client-a", "nonce-abc")).await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// GET requests have no replay risk and must bypass nonce checking entirely.
// The mock is configured to reject, so any check would produce a 409.
#[tokio::test]
async fn get_request_skips_nonce_check() {
    let app = make_app(Ok(false));
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// The state-init endpoint has no outerRequestJws and must bypass nonce checking.
// The mock is configured to reject, so any check would produce a 409.
#[tokio::test]
async fn state_init_endpoint_skips_nonce_check() {
    let app = make_app(Ok(false));
    let body = serde_json::json!({
        "clientJwsPublicKey": { "kty": "EC", "crv": "P-256", "x": "x", "y": "y", "kid": "" },
        "clientJwePublicKey": { "kty": "EC", "crv": "P-256", "x": "x", "y": "y", "kid": "" }
    })
    .to_string();
    let resp = post_to(app, "/hsm/v1/device-states", body).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn malformed_jws_returns_400() {
    let app = make_app(Ok(true));
    let body = serde_json::json!({
        "clientId": "client-a",
        "outerRequestJws": "not-a-jws"
    })
    .to_string();
    let resp = post_to(app, "/test", body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn missing_nonce_field_returns_400() {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256"}"#);
    // Valid JWS structure but payload contains no 'nonce' field
    let payload = URL_SAFE_NO_PAD.encode(r#"{"version":1}"#);
    let sig = URL_SAFE_NO_PAD.encode(b"fakesig");
    let jws = format!("{}.{}.{}", header, payload, sig);

    let app = make_app(Ok(true));
    let body = serde_json::json!({
        "clientId": "client-a",
        "outerRequestJws": jws
    })
    .to_string();
    let resp = post_to(app, "/test", body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
