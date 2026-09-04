// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod handlers;
pub mod replay_protection;

use axum::Json;
use axum::Router;
use axum::http::{StatusCode, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::Counter;
use opentelemetry_http::HeaderExtractor;
use std::sync::Arc;
use std::sync::OnceLock;
use tower_http::trace::TraceLayer;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// Lazy-init counter so the global meter provider (set by
/// `infrastructure::telemetry::init`) is in place before we build the
/// Counter. Bumped per HTTP request in the TraceLayer below.
fn http_requests_counter() -> &'static Counter<u64> {
    static C: OnceLock<Counter<u64>> = OnceLock::new();
    C.get_or_init(|| {
        global::meter("wallet-bff")
            .u64_counter("http.server.requests")
            .with_description("HTTP requests received by wallet-bff")
            .build()
    })
}

use crate::domain::ProblemDetail;

use handlers::{AppState, problem_content_type_header};
use replay_protection::ReplayProtectionState;

/// RFC 9457 Problem Details response with Content-Type: application/problem+json.
pub(super) fn problem_response(
    status: StatusCode,
    title: &str,
    detail: Option<&str>,
    instance: &str,
) -> Response {
    build_problem_response(status, title, detail, instance, None)
}

/// Like [`problem_response`] but includes a `requestId` field for traceability.
pub(super) fn tracked_problem_response(
    status: StatusCode,
    title: &str,
    detail: Option<&str>,
    instance: &str,
    request_id: &str,
) -> Response {
    build_problem_response(status, title, detail, instance, Some(request_id))
}

/// 503 ProblemDetail for transient upstream data-store outages
/// (e.g. Valkey reconnect exhausted after `with_redis_retry`).
/// Detail intentionally omits internals per SAK.25/26 — the adapter
/// logs the underlying error separately.
pub(super) fn service_unavailable_problem(instance: &str, detail: &str) -> Response {
    build_problem_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "Service Unavailable",
        Some(detail),
        instance,
        None,
    )
}

fn build_problem_response(
    status: StatusCode,
    title: &str,
    detail: Option<&str>,
    instance: &str,
    request_id: Option<&str>,
) -> Response {
    let body = ProblemDetail {
        problem_type: None,
        title: title.to_string(),
        status: status.as_u16(),
        detail: detail.map(str::to_string),
        instance: Some(instance.to_string()),
        request_id: request_id.map(str::to_string),
    };
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, problem_content_type_header().clone());
    response
}

#[derive(OpenApi)]
#[openapi(
    info(title = "wallet-bff", version = "0.1.0"),
    paths(handlers::task_response, handlers::service, handlers::create_state,),
    components(schemas(
        crate::domain::BffRequest,
        crate::domain::NewStateRequestDto,
        crate::domain::NewStateResponseDto,
        crate::domain::AsyncResponseDto,
        crate::domain::AsyncResponseStatus,
        crate::domain::EcPublicJwk,
        crate::domain::ProblemDetail,
    ))
)]
struct ApiDoc;

pub fn router(state: Arc<AppState>, rp_state: Arc<ReplayProtectionState>) -> Router {
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/hsm/v1/openapi.json", ApiDoc::openapi()))
        .route(
            "/hsm/v1/requests/{correlationId}",
            get(handlers::task_response),
        )
        .route("/hsm/v1/requests", post(handlers::service))
        .route("/hsm/v1/device-states", post(handlers::create_state))
        .layer(middleware::from_fn_with_state(
            rp_state,
            replay_protection::replay_protection,
        ))
        // Outermost layer — emits a tracing span per request that the
        // OpenTelemetry layer (registered in infrastructure::telemetry)
        // exports as an OTLP span. Captures method, URI for every
        // endpoint without per-handler annotations.
        //
        // make_span_with extracts the W3C tracecontext from incoming
        // headers (set by the istio-proxy sidecar) and sets it as the
        // parent of the app span. Result: app span and envoy span share
        // the same trace_id, so Tempo / Kiali render the full request
        // path as one trace.
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                let parent_cx = global::get_text_map_propagator(|propagator| {
                    propagator.extract(&HeaderExtractor(request.headers()))
                });
                http_requests_counter().add(
                    1,
                    &[KeyValue::new("http.method", request.method().to_string())],
                );
                let span = tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                );

                if let Err(err) = span.set_parent(parent_cx) {
                    tracing::warn!(?err, "Failed to set parent_ctx");
                }
                span
            }),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn build_problem_response_sets_status_and_problem_json_content_type() {
        let response = build_problem_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service Unavailable",
            Some("Nonce store is temporarily unavailable. Please retry."),
            "/hsm/v1/requests",
            None,
        );

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );

        let body = body_json(response).await;
        assert_eq!(body["title"], "Service Unavailable");
        assert_eq!(body["status"], 503);
        assert_eq!(
            body["detail"],
            "Nonce store is temporarily unavailable. Please retry."
        );
        assert_eq!(body["instance"], "/hsm/v1/requests");
    }

    #[tokio::test]
    async fn build_problem_response_omits_absent_optional_fields_from_json() {
        let response = build_problem_response(
            StatusCode::NOT_FOUND,
            "Not Found",
            None,
            "/hsm/v1/device-states",
            None,
        );

        let body = body_json(response).await;
        // detail/request_id/type use skip_serializing_if, so absent means
        // the key itself is missing from the JSON, not present-with-null.
        assert!(!body.as_object().unwrap().contains_key("detail"));
        assert!(!body.as_object().unwrap().contains_key("request_id"));
        assert!(!body.as_object().unwrap().contains_key("type"));
    }

    #[tokio::test]
    async fn build_problem_response_includes_request_id_when_tracked() {
        let response = build_problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some("boom"),
            "/hsm/v1/requests",
            Some("corr-123"),
        );

        let body = body_json(response).await;
        assert_eq!(body["request_id"], "corr-123");
    }
}
