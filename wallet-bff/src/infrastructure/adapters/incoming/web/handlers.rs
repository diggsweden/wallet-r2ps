// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::Json;
use axum::extract::{OriginalUri, Path, State, rejection::JsonRejection};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use uuid::Uuid;

use super::problem_response;
use crate::application::port::incoming::ResponseUseCase;
use crate::application::port::outgoing::{
    DeviceStatePort, RequestSenderPort, StateInitCorrelationPort, StateInitSenderPort,
};
use crate::domain::{
    AsyncResponseDto, AsyncResponseStatus, BffRequest, CachedResponse, DEFAULT_TTL_SECONDS,
    HsmWorkerRequest, NewStateRequestDto, NewStateResponseDto, ProblemDetail, StateInitRequest,
    TypedJws,
};

pub const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";

pub struct AppState {
    pub device_state_port: Arc<dyn DeviceStatePort>,
    pub request_sender_port: Arc<dyn RequestSenderPort>,
    pub state_init_sender_port: Arc<dyn StateInitSenderPort>,
    pub response_use_case: Arc<dyn ResponseUseCase>,
    pub state_init_correlation: Arc<dyn StateInitCorrelationPort>,
    pub serve_sync: bool,
    pub sync_timeout_ms: u64,
    pub state_init_timeout_ms: u64,
    pub response_events_template_url: String,
}

impl AppState {
    fn polling_url(&self, correlation_id: &str) -> String {
        self.response_events_template_url
            .replace("%s", correlation_id)
    }
}

/// Returns a pre-built RFC 9457 JSON string (from the worker) as a problem+json response.
fn forward_problem_response(status: StatusCode, raw_json: String) -> axum::response::Response {
    let mut response = (status, raw_json).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, PROBLEM_CONTENT_TYPE.parse().unwrap());
    response
}

/// Maps a JsonRejection to the appropriate RFC 9457 status + messages (SAK.25/26: no internals).
fn json_rejection_response(e: JsonRejection, instance: &str) -> axum::response::Response {
    let (status, title, detail) = match &e {
        JsonRejection::MissingJsonContentType(_) => (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported Media Type",
            "Content-Type header must be 'application/json'.",
        ),
        JsonRejection::JsonDataError(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Unprocessable Entity",
            "The request body does not conform to the expected schema.",
        ),
        JsonRejection::JsonSyntaxError(_) => (
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "The request body contains malformed JSON.",
        ),
        _ => (
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "The request body could not be processed.",
        ),
    };
    problem_response(status, title, Some(detail), instance)
}

/// GET /hsm/requests/{correlationId}
#[utoipa::path(
    get,
    path = "/hsm/v1/requests/{correlationId}",
    params(("correlationId" = Uuid, Path, description = "Correlation ID returned by a prior POST /hsm/requests")),
    responses(
        (status = 200, description = "Request completed", body = AsyncResponseDto),
        (status = 202, description = "Request still pending", body = AsyncResponseDto),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
    )
)]
pub async fn task_response(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Path(correlation_id): Path<Uuid>,
) -> impl IntoResponse {
    let id_str = correlation_id.to_string();
    let cached = state
        .response_use_case
        .wait_for_response(&id_str, state.sync_timeout_ms)
        .await;
    let polling_url = state.polling_url(&id_str);

    build_async_response(correlation_id, cached, polling_url, uri.path())
}

/// POST /hsm/requests
#[utoipa::path(
    post,
    path = "/hsm/v1/requests",
    request_body(content = BffRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Request completed synchronously", body = AsyncResponseDto),
        (status = 202, description = "Request accepted, poll for result", body = AsyncResponseDto),
        (status = 404, description = "Device state not found", body = ProblemDetail, content_type = "application/problem+json"),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
    )
)]
pub async fn service(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    body: Result<Json<BffRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(req) = match body {
        Ok(j) => j,
        Err(e) => return json_rejection_response(e, uri.path()),
    };
    let instance = uri.path().to_string();

    let state_jws = match state.device_state_port.load(&req.client_id).await {
        Some(s) => s,
        None => {
            info!("No state found for clientId: {}", req.client_id);
            return problem_response(
                StatusCode::NOT_FOUND,
                "Device Not Found",
                Some(&format!(
                    "No device state found for clientId: {}",
                    req.client_id
                )),
                &instance,
            );
        }
    };

    let request_id = Uuid::new_v4();
    let request_id_str = request_id.to_string();

    // Register the pending entry before sending to Kafka so the response can
    // never arrive before we are ready to receive it.
    let rx = state.response_use_case.register_pending(
        &request_id_str,
        &req.client_id,
        DEFAULT_TTL_SECONDS,
    );

    let worker_req = HsmWorkerRequest {
        request_id: request_id_str.clone(),
        state_jws,
        outer_request_jws: TypedJws::new(req.outer_request_jws),
        // response_topic is injected by the Kafka sender
        response_topic: String::new(),
    };

    if let Err(e) = state
        .request_sender_port
        .send(&worker_req, &req.client_id)
        .await
    {
        tracing::error!("Failed to send worker request: {}", e);
        return problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some("Failed to enqueue the request."),
            &instance,
        );
    }

    if state.serve_sync {
        info!(
            "Waiting for synchronous response for requestId: {}",
            request_id_str
        );
        let cached = tokio::time::timeout(Duration::from_millis(state.sync_timeout_ms), async {
            rx.await.ok()
        })
        .await
        .ok()
        .flatten();
        let polling_url = state.polling_url(&request_id_str);
        return build_async_response(request_id, cached, polling_url, &instance);
    }

    let location = state.polling_url(&request_id_str);
    let body = AsyncResponseDto {
        correlation_id: request_id,
        status: AsyncResponseStatus::Pending,
        result: None,
        result_url: Some(location.clone()),
        error: None,
    };
    let mut headers = HeaderMap::new();
    if let Ok(v) = location.parse() {
        headers.insert(header::LOCATION, v);
    }
    (StatusCode::ACCEPTED, headers, Json(body)).into_response()
}

/// POST /hsm/v1/operations (synchronous endpoint)
#[utoipa::path(
    post,
    path = "/hsm/v1/operations",
    request_body(content = BffRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Worker result (plain JWS string)"),
        (status = 408, description = "Request timeout", body = ProblemDetail, content_type = "application/problem+json"),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
    )
)]
pub async fn legacy_service(
    state: State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    body: Result<Json<BffRequest>, JsonRejection>,
) -> impl IntoResponse {
    let response = service(state, OriginalUri(uri.clone()), body)
        .await
        .into_response();
    let status = response.status();

    let bytes = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(b) => b,
        Err(_) => {
            return problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                Some("Failed to read response body."),
                uri.path(),
            );
        }
    };

    let dto: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            return problem_response(
                StatusCode::REQUEST_TIMEOUT,
                "Request Timeout",
                Some("No response received within the timeout period."),
                uri.path(),
            );
        }
    };

    if let Some(result) = dto.get("result").and_then(|v| v.as_str()) {
        return (status, result.to_string()).into_response();
    }

    problem_response(
        StatusCode::REQUEST_TIMEOUT,
        "Request Timeout",
        Some("No response received within the timeout period."),
        uri.path(),
    )
}

/// POST /hsm/v1/device-states
#[utoipa::path(
    post,
    path = "/hsm/v1/device-states",
    request_body(content = NewStateRequestDto, content_type = "application/json"),
    responses(
        (status = 200, description = "State initialized successfully", body = NewStateResponseDto),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
    )
)]
pub async fn create_state(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    body: Result<Json<NewStateRequestDto>, JsonRejection>,
) -> impl IntoResponse {
    let Json(req) = match body {
        Ok(j) => j,
        Err(e) => return json_rejection_response(e, uri.path()).into_response(),
    };

    let instance = uri.path().to_string();

    let ttl_seconds = req
        .ttl
        .as_deref()
        .and_then(parse_iso8601_to_seconds)
        .unwrap_or(DEFAULT_TTL_SECONDS);

    let client_id = if req.overwrite {
        req.client_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string())
    } else {
        Uuid::new_v4().to_string()
    };

    if !req.overwrite
        && let Some(_existing) = state.device_state_port.load(&client_id).await
    {
        let dto = NewStateResponseDto {
            status: "OK".to_string(),
            client_id,
            dev_authorization_code: None,
            server_jws_public_key: None,
            opaque_server_id: None,
        };
        return Json(dto).into_response();
    }

    let request_id = Uuid::new_v4().to_string();

    // Register before sending to Kafka.
    let rx = state
        .state_init_correlation
        .register_pending(&request_id, &client_id, ttl_seconds)
        .await;

    let init_request = StateInitRequest {
        request_id: request_id.clone(),
        public_key: req.public_key,
        // response_topic is injected by the Kafka sender
        response_topic: String::new(),
    };

    if let Err(e) = state
        .state_init_sender_port
        .send(&init_request, &client_id)
        .await
    {
        tracing::error!("Failed to send state-init request: {}", e);
        return problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some("Failed to enqueue state initialization."),
            &instance,
        )
        .into_response();
    }

    info!(
        "Sent state-init request for clientId: {}, requestId: {}",
        client_id, request_id
    );

    let init_response =
        tokio::time::timeout(Duration::from_millis(state.state_init_timeout_ms), async {
            rx.await.ok()
        })
        .await
        .ok()
        .flatten();

    match init_response {
        Some(resp) => {
            info!(
                "New state created for clientId: {}, dev_authorization_code: {}",
                client_id, resp.dev_authorization_code
            );
            let dto = NewStateResponseDto {
                status: "OK".to_string(),
                client_id,
                dev_authorization_code: Some(resp.dev_authorization_code),
                server_jws_public_key: resp.server_jws_public_key,
                opaque_server_id: resp.opaque_server_id,
            };
            Json(dto).into_response()
        }
        None => {
            tracing::error!("State initialization timeout for clientId: {}", client_id);
            problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                Some("State initialization did not complete within the expected time."),
                &instance,
            )
            .into_response()
        }
    }
}

pub fn build_async_response(
    correlation_id: Uuid,
    cached: Option<CachedResponse>,
    polling_url: String,
    instance: &str,
) -> axum::response::Response {
    match cached {
        None => {
            let body = AsyncResponseDto {
                correlation_id,
                status: AsyncResponseStatus::Pending,
                result: None,
                result_url: Some(polling_url.clone()),
                error: None,
            };
            let mut headers = HeaderMap::new();
            if let Ok(v) = polling_url.parse() {
                headers.insert(header::LOCATION, v);
            }
            (StatusCode::ACCEPTED, headers, Json(body)).into_response()
        }
        Some(resp) if resp.status != hsm_common::Status::Ok => {
            // Forward the worker's pre-built RFC 9457 JSON if available.
            if let Some(problem_response_str) = resp.error_message {
                return forward_problem_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    problem_response_str,
                );
            }
            problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                Some("The worker returned a non-OK status."),
                instance,
            )
        }
        Some(resp) => {
            let body = AsyncResponseDto {
                correlation_id,
                status: AsyncResponseStatus::Complete,
                result: resp.outer_response_jws.map(|j| j.into_string()),
                result_url: None,
                error: None,
            };
            Json(body).into_response()
        }
    }
}

pub fn parse_iso8601_to_seconds(iso: &str) -> Option<u64> {
    iso.parse::<iso8601_duration::Duration>().ok().map(|d| {
        let secs = d.year * 365.25 * 86400.0
            + d.month * 30.44 * 86400.0
            + d.day * 86400.0
            + d.hour * 3600.0
            + d.minute * 60.0
            + d.second;
        secs as u64
    })
}
