// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::Json;
use axum::extract::{OriginalUri, Path, State, rejection::JsonRejection};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use std::sync::{Arc, OnceLock};
use tracing::{info, warn};
use uuid::Uuid;

use super::{problem_response, tracked_problem_response};
use crate::application::port::outgoing::{
    DeviceStatePort, RequestContextPort, RequestSenderPort, ResponseStorePort, StateInitSenderPort,
};
use crate::application::service::{hsm_response_key, state_init_response_key};
use crate::domain::{
    AsyncResponseDto, AsyncResponseStatus, BffRequest, CachedResponse, CachedStateInitResponse,
    Curve, DEFAULT_TTL_SECONDS, HsmWorkerRequest, NewStateRequestDto, NewStateResponseDto,
    ProblemDetail, RequestContext, StateInitRequest, TypedJws,
};

pub const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";

static PROBLEM_CONTENT_TYPE_HEADER: OnceLock<HeaderValue> = OnceLock::new();

pub(super) fn problem_content_type_header() -> &'static HeaderValue {
    PROBLEM_CONTENT_TYPE_HEADER
        .get_or_init(|| PROBLEM_CONTENT_TYPE.parse().expect("valid content type"))
}

pub struct AppState {
    pub device_state_port: Arc<dyn DeviceStatePort>,
    pub request_sender_port: Arc<dyn RequestSenderPort>,
    pub state_init_sender_port: Arc<dyn StateInitSenderPort>,
    pub response_store: Arc<dyn ResponseStorePort>,
    pub request_context_port: Arc<dyn RequestContextPort>,
    pub long_poll_timeout_seconds: u64,
    pub response_events_template_url: String,
    pub state_init_events_template_url: String,
    pub default_initial_key_curve: Curve,
}

impl AppState {
    fn request_polling_url(&self, correlation_id: &str) -> String {
        self.response_events_template_url
            .replace("%s", correlation_id)
    }

    fn state_init_polling_url(&self, correlation_id: &str) -> String {
        self.state_init_events_template_url
            .replace("%s", correlation_id)
    }
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

/// GET /hsm/v1/requests/{correlationId}
#[utoipa::path(
    get,
    path = "/hsm/v1/requests/{correlationId}",
    params(("correlationId" = Uuid, Path, description = "Correlation ID returned by a prior POST /hsm/requests")),
    responses(
        (status = 200, description = "Request completed", body = AsyncResponseDto),
        (status = 202, description = "Request still pending — keep polling", body = AsyncResponseDto,
            headers(("Location" = String, description = "URL to poll for the result"))),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
        (status = "default", description = "Unexpected error", body = ProblemDetail, content_type = "application/problem+json"),
    )
)]
pub async fn task_response(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Path(correlation_id): Path<Uuid>,
) -> impl IntoResponse {
    let id_str = correlation_id.to_string();
    let polling_url = state.request_polling_url(&id_str);
    let key = hsm_response_key(&id_str);

    let bytes = match state
        .response_store
        .await_value(&key, state.long_poll_timeout_seconds)
        .await
    {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Response store error for {}: {}", id_str, e);
            return tracked_problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                Some("Failed to read response store."),
                uri.path(),
                &id_str,
            );
        }
    };

    let cached: Option<CachedResponse> = bytes.and_then(|b| match serde_json::from_slice(&b) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::error!("Failed to decode cached response for {}: {}", id_str, e);
            None
        }
    });

    build_async_response(correlation_id, cached, polling_url, uri.path())
}

/// POST /hsm/v1/requests — fully async: enqueue and return 202 with Location.
#[utoipa::path(
    post,
    path = "/hsm/v1/requests",
    request_body(content = BffRequest, content_type = "application/json"),
    responses(
        (status = 202, description = "Request accepted, poll for result", body = AsyncResponseDto,
            headers(("Location" = String, description = "URL to poll for the result"))),
        (status = 404, description = "Device state not found", body = ProblemDetail, content_type = "application/problem+json"),
        (status = 409, description = "Duplicate nonce — generate a new nonce and retry", body = ProblemDetail, content_type = "application/problem+json"),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
        (status = "default", description = "Unexpected error", body = ProblemDetail, content_type = "application/problem+json"),
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

    let state_jws = match req.state_jws {
        Some(s) => s,
        None => match state.device_state_port.load(&req.client_id).await {
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
        },
    };

    let request_id = Uuid::new_v4();
    let request_id_str = request_id.to_string();

    // Persist {request_id -> client_id} BEFORE Kafka send so the response
    // consumer — possibly on a different replica — can locate this device.
    let ctx = RequestContext {
        client_id: req.client_id.clone(),
        ttl_seconds: DEFAULT_TTL_SECONDS,
    };
    if let Err(e) = state
        .request_context_port
        .store(&request_id_str, &ctx, DEFAULT_TTL_SECONDS)
        .await
    {
        tracing::error!("Failed to store request context: {}", e);
        return tracked_problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some("Failed to persist request context."),
            &instance,
            &request_id_str,
        );
    }

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
        return tracked_problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some("Failed to enqueue the request."),
            &instance,
            &request_id_str,
        );
    }

    let location = state.request_polling_url(&request_id_str);
    let body = AsyncResponseDto {
        correlation_id: request_id,
        status: AsyncResponseStatus::Pending,
        result: None,
        result_url: Some(location.clone()),
        state_jws: None,
    };
    let mut headers = HeaderMap::new();
    if let Ok(v) = location.parse() {
        headers.insert(header::LOCATION, v);
    }
    (StatusCode::ACCEPTED, headers, Json(body)).into_response()
}

/// POST /hsm/v1/device-states — fully async: enqueue state-init and return 202.
#[utoipa::path(
    post,
    path = "/hsm/v1/device-states",
    request_body(content = NewStateRequestDto, content_type = "application/json"),
    responses(
        (status = 202, description = "State init accepted, poll the Location URL for the result", body = AsyncResponseDto,
            headers(("Location" = String, description = "URL to poll for the result"))),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
        (status = "default", description = "Unexpected error", body = ProblemDetail, content_type = "application/problem+json"),
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

    let request_id = Uuid::new_v4().to_string();

    let ctx = RequestContext {
        client_id: client_id.clone(),
        ttl_seconds,
    };
    if let Err(e) = state
        .request_context_port
        .store(&request_id, &ctx, ttl_seconds)
        .await
    {
        tracing::error!("Failed to store state-init request context: {}", e);
        return tracked_problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some("Failed to persist request context."),
            &instance,
            &request_id,
        )
        .into_response();
    }

    let init_request = StateInitRequest {
        request_id: request_id.clone(),
        public_key: req.public_key,
        // response_topic is injected by the Kafka sender
        response_topic: String::new(),
        initial_key_curve: req
            .initial_key_curve
            .unwrap_or(state.default_initial_key_curve),
    };

    if let Err(e) = state
        .state_init_sender_port
        .send(&init_request, &client_id)
        .await
    {
        tracing::error!("Failed to send state-init request: {}", e);
        return tracked_problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some("Failed to enqueue state initialization."),
            &instance,
            &request_id,
        )
        .into_response();
    }

    info!(
        "Sent state-init request for clientId: {}, requestId: {}",
        client_id, request_id
    );

    let location = state.state_init_polling_url(&request_id);
    let correlation_id = match Uuid::parse_str(&request_id) {
        Ok(u) => u,
        Err(_) => Uuid::nil(),
    };
    let body = AsyncResponseDto {
        correlation_id,
        status: AsyncResponseStatus::Pending,
        result: None,
        result_url: Some(location.clone()),
        state_jws: None,
    };
    let mut headers = HeaderMap::new();
    if let Ok(v) = location.parse() {
        headers.insert(header::LOCATION, v);
    }
    (StatusCode::ACCEPTED, headers, Json(body)).into_response()
}

/// GET /hsm/v1/device-states/{correlationId} — long-poll for state-init.
#[utoipa::path(
    get,
    path = "/hsm/v1/device-states/{correlationId}",
    params(("correlationId" = Uuid, Path, description = "Correlation ID returned by POST /hsm/v1/device-states")),
    responses(
        (status = 200, description = "State init completed", body = NewStateResponseDto),
        (status = 202, description = "State init still pending — keep polling", body = AsyncResponseDto,
            headers(("Location" = String, description = "URL to poll for the result"))),
        (status = 500, description = "Internal server error", body = ProblemDetail, content_type = "application/problem+json"),
        (status = "default", description = "Unexpected error", body = ProblemDetail, content_type = "application/problem+json"),
    )
)]
pub async fn state_init_response(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Path(correlation_id): Path<Uuid>,
) -> impl IntoResponse {
    let id_str = correlation_id.to_string();
    let polling_url = state.state_init_polling_url(&id_str);
    let key = state_init_response_key(&id_str);

    let bytes = match state
        .response_store
        .await_value(&key, state.long_poll_timeout_seconds)
        .await
    {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Response store error for state-init {}: {}", id_str, e);
            return tracked_problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                Some("Failed to read response store."),
                uri.path(),
                &id_str,
            );
        }
    };

    let Some(bytes) = bytes else {
        return state_init_pending(correlation_id, polling_url);
    };

    let cached: CachedStateInitResponse = match serde_json::from_slice(&bytes) {
        Ok(c) => c,
        Err(e) => {
            warn!(
                "Failed to decode cached state-init response for {}: {}",
                id_str, e
            );
            return tracked_problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                Some("Failed to decode response."),
                uri.path(),
                &id_str,
            );
        }
    };

    let dto = NewStateResponseDto {
        status: "OK".to_string(),
        client_id: cached.client_id,
        dev_authorization_code: Some(cached.dev_authorization_code),
        server_jws_public_key: cached.server_jws_public_key,
        opaque_server_id: cached.opaque_server_id,
        state_jws: Some(cached.state_jws),
    };
    Json(dto).into_response()
}

fn state_init_pending(correlation_id: Uuid, polling_url: String) -> axum::response::Response {
    let body = AsyncResponseDto {
        correlation_id,
        status: AsyncResponseStatus::Pending,
        result: None,
        result_url: Some(polling_url.clone()),
        state_jws: None,
    };
    let mut headers = HeaderMap::new();
    if let Ok(v) = polling_url.parse() {
        headers.insert(header::LOCATION, v);
    }
    (StatusCode::ACCEPTED, headers, Json(body)).into_response()
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
                state_jws: None,
            };
            let mut headers = HeaderMap::new();
            if let Ok(v) = polling_url.parse() {
                headers.insert(header::LOCATION, v);
            }
            (StatusCode::ACCEPTED, headers, Json(body)).into_response()
        }
        Some(resp) if resp.status != hsm_common::Status::Ok => {
            let worker_detail = resp
                .error_message
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v["detail"].as_str().map(str::to_string))
                .unwrap_or_else(|| "Worker returned a non-OK status".to_string());
            let body = ProblemDetail {
                problem_type: None,
                title: "Internal Server Error".to_string(),
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                detail: Some(worker_detail),
                instance: Some(instance.to_string()),
                request_id: Some(correlation_id.to_string()),
            };
            let mut response = (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, problem_content_type_header().clone());
            response
        }
        Some(resp) => {
            let body = AsyncResponseDto {
                correlation_id,
                status: AsyncResponseStatus::Complete,
                result: resp.outer_response_jws.map(|j| j.into_string()),
                result_url: None,
                state_jws: resp.state_jws,
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
