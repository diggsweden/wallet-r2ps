// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::body::to_bytes;
use axum::http::{StatusCode, header};
use rstest::rstest;
use uuid::Uuid;
use wallet_bff::domain::{CachedResponse, OuterResponse, Status, TypedJws};
use wallet_bff::infrastructure::adapters::incoming::web::handlers::{
    PROBLEM_CONTENT_TYPE, build_async_response, parse_iso8601_to_seconds,
};

// ── parse_iso8601_to_seconds ─────────────────────────────────────────────

#[rstest]
#[case("P1D", 86_400)]
#[case("PT1H", 3_600)]
#[case("PT1M", 60)]
#[case("PT1S", 1)]
#[case("P1Y", 31_557_600)] // 365.25 * 86400
#[case("P1M", 2_630_016)] // 30.44 * 86400
#[case("P1DT2H3M", 93_780)] // 86400 + 7200 + 180
fn parse_iso8601_valid(#[case] input: &str, #[case] expected: u64) {
    assert_eq!(parse_iso8601_to_seconds(input), Some(expected));
}

#[rstest]
#[case("not-a-duration")]
#[case("")]
#[case("P")]
fn parse_iso8601_invalid_returns_none(#[case] input: &str) {
    assert_eq!(parse_iso8601_to_seconds(input), None);
}

// ── build_async_response ─────────────────────────────────────────────────

#[tokio::test]
async fn build_async_response_none_returns_202_pending() {
    let id = Uuid::new_v4();
    let resp = build_async_response(id, None, "http://poll/1".to_string(), "/test");

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    assert_eq!(
        resp.headers().get(header::LOCATION).unwrap(),
        "http://poll/1"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["status"], "pending");
    assert_eq!(body["resultUrl"], "http://poll/1");
    assert!(body.get("result").is_none());
}

#[tokio::test]
async fn build_async_response_ok_returns_200_complete() {
    let id = Uuid::new_v4();
    let cached = CachedResponse {
        request_id: id.to_string(),
        status: Status::Ok,
        outer_response_jws: Some(TypedJws::<OuterResponse>::new("jws.token.here".to_string())),
        state_jws: None,
        error_message: None,
    };
    let resp = build_async_response(id, Some(cached), "http://poll/1".to_string(), "/test");

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["status"], "complete");
    assert_eq!(body["result"], "jws.token.here");
    assert!(body.get("resultUrl").is_none());
}

#[rstest]
#[case(Some(r#"{"title":"Worker error","status":500}"#.to_string()))]
#[case(None)]
#[tokio::test]
async fn build_async_response_error_returns_500(#[case] error_message: Option<String>) {
    let id = Uuid::new_v4();
    let cached = CachedResponse {
        request_id: id.to_string(),
        status: Status::Error,
        outer_response_jws: None,
        state_jws: None,
        error_message,
    };
    let resp = build_async_response(id, Some(cached), "http://poll/1".to_string(), "/test");

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        PROBLEM_CONTENT_TYPE
    );
}

#[tokio::test]
async fn build_async_response_forwards_worker_problem_json_exactly() {
    let id = Uuid::new_v4();
    // A realistic RFC 9457 problem detail as produced by the worker service
    let worker_error = format!(
        r#"{{"title":"Error processing request","detail":"UnknownDevice","request_id":"{}"}}"#,
        id
    );
    let cached = CachedResponse {
        request_id: id.to_string(),
        status: Status::Error,
        outer_response_jws: None,
        state_jws: None,
        error_message: Some(worker_error.clone()),
    };
    let resp = build_async_response(id, Some(cached), "http://poll".into(), "/test");

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        PROBLEM_CONTENT_TYPE
    );
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes, worker_error);
}
