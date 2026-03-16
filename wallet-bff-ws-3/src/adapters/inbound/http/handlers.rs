use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use uuid::Uuid;

use super::dto::{
    ApiInfoDto, AsyncResponseDto, AsyncResponseError, BffRequest, NewStateRequestDto,
    NewStateResponseDto,
};
use super::state::HttpAppState;
use crate::application::request_processing::{PollResult, SubmitResult};
use crate::domain::{
    device_management::value_objects::EcPublicKey,
    request_processing::value_objects::ProcessingMode,
};
use crate::error::{AppError, Result};

/// POST / - Main service request endpoint
///
/// Accepts a service request and either returns immediately with a pending status
/// (async mode) or waits for the result (sync mode).
#[utoipa::path(
    post,
    path = "/",
    request_body = BffRequest,
    responses(
        (status = 200, description = "Request completed successfully (sync mode)", body = AsyncResponseDto<String>),
        (status = 202, description = "Request accepted and pending (async mode)", body = AsyncResponseDto<String>),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Device not found"),
        (status = 408, description = "Request timeout (sync mode)")
    ),
    tag = "R2PS API"
)]
pub async fn submit_request(
    State(state): State<Arc<HttpAppState>>,
    Json(request): Json<BffRequest>,
) -> Result<impl IntoResponse> {
    let mode = if state.serve_sync {
        ProcessingMode::Synchronous
    } else {
        ProcessingMode::Asynchronous
    };

    let result = state
        .submit_request_use_case
        .execute(&request.client_id, &request.outer_request_jws, None, mode)
        .await
        .map_err(AppError::from_request_error)?;

    let base_url = &state.base_url;
    match result {
        SubmitResult::Pending { correlation_id } => {
            let cid = correlation_id.as_uuid();
            let result_url = format!("{}/requests/{}", base_url, cid);
            let dto = AsyncResponseDto::<String>::pending(cid, result_url, base_url);
            Ok((StatusCode::ACCEPTED, Json(dto)).into_response())
        }
        SubmitResult::Complete {
            correlation_id,
            response_jws,
        } => {
            let cid = correlation_id.as_uuid();
            let dto = AsyncResponseDto::complete(cid, response_jws, base_url);
            Ok((StatusCode::OK, Json(dto)).into_response())
        }
        SubmitResult::Failed {
            correlation_id,
            http_status,
            message,
        } => {
            let cid = correlation_id.as_uuid();
            let error = AsyncResponseError {
                message,
                http_status,
            };
            let dto = AsyncResponseDto::<String>::error(cid, error, base_url);
            Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(dto)).into_response())
        }
    }
}

/// GET /requests/{correlationId} - Response polling endpoint
///
/// Poll for the result of an async request using the correlation ID.
#[utoipa::path(
    get,
    path = "/requests/{correlationId}",
    params(
        ("correlationId" = Uuid, Path, description = "Correlation ID of the pending request")
    ),
    responses(
        (status = 200, description = "Request completed", body = AsyncResponseDto<String>),
        (status = 202, description = "Request still pending", body = AsyncResponseDto<String>),
        (status = 404, description = "Request not found")
    ),
    tag = "R2PS API"
)]
pub async fn poll_task(
    State(state): State<Arc<HttpAppState>>,
    Path(correlation_id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let cid =
        crate::domain::request_processing::value_objects::CorrelationId::from_uuid(correlation_id);

    let result = state
        .poll_request_use_case
        .execute(cid)
        .await
        .map_err(AppError::from_request_error)?;

    let base_url = &state.base_url;
    match result {
        PollResult::Pending { correlation_id } => {
            let uuid = correlation_id.as_uuid();
            let result_url = format!("{}/requests/{}", base_url, uuid);
            let dto = AsyncResponseDto::<String>::pending(uuid, result_url, base_url);
            Ok((StatusCode::ACCEPTED, Json(dto)).into_response())
        }
        PollResult::Complete {
            correlation_id,
            response_jws,
        } => {
            let uuid = correlation_id.as_uuid();
            let dto = AsyncResponseDto::complete(uuid, response_jws, base_url);
            Ok((StatusCode::OK, Json(dto)).into_response())
        }
        PollResult::Failed {
            correlation_id,
            http_status,
            message,
        } => {
            let uuid = correlation_id.as_uuid();
            let status =
                StatusCode::from_u16(http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let error = AsyncResponseError {
                message,
                http_status,
            };
            let dto = AsyncResponseDto::<String>::error(uuid, error, base_url);
            Ok((status, Json(dto)).into_response())
        }
    }
}

/// POST /device-states - Device state initialization endpoint
///
/// Initialize a new device with a public key and obtain a client ID.
/// Device state is managed server-side by the HSM worker in PostgreSQL.
/// The response contains the `service_response_jws` which includes the
/// encrypted `InnerResponse` with the `dev_authorization_code`.
///
/// **WARNING**: Contains dev-only features (overwrite, clientId) that should be removed for production.
#[utoipa::path(
    post,
    path = "/device-states",
    request_body = NewStateRequestDto,
    responses(
        (status = 200, description = "Device initialized successfully", body = NewStateResponseDto),
        (status = 400, description = "Invalid request"),
        (status = 408, description = "Timeout waiting for HSM response")
    ),
    tag = "R2PS API"
)]
pub async fn new_state(
    State(state): State<Arc<HttpAppState>>,
    Json(request): Json<NewStateRequestDto>,
) -> Result<impl IntoResponse> {
    // Map DTO -> domain value object
    let public_key = EcPublicKey::new(
        request.public_key.kty,
        request.public_key.crv,
        request.public_key.x,
        request.public_key.y,
        request.public_key.kid,
    )
    .map_err(|e| AppError::InvalidRequest(e.to_string()))?;

    let result = state
        .init_device_use_case
        .execute(
            public_key,
            request.client_id,
            request.overwrite.unwrap_or(false),
        )
        .await
        .map_err(AppError::from_device_error)?;

    let dto = NewStateResponseDto::new(
        result.client_id.to_string(),
        result.service_response_jws,
        &state.base_url,
    );

    Ok((StatusCode::OK, Json(dto)))
}

/// GET /api-info - API Information endpoint
///
/// Returns API metadata as required by Swedish REST API Profile (VER.06, VER.07).
#[utoipa::path(
    get,
    path = "/api-info",
    responses(
        (status = 200, description = "API information", body = ApiInfoDto)
    ),
    tag = "API Metadata"
)]
pub async fn api_info() -> impl IntoResponse {
    let info = ApiInfoDto {
        api_name: "r2ps-api".to_string(),
        api_version: env!("CARGO_PKG_VERSION").to_string(),
        api_released: "2024-01-01".to_string(),
        api_documentation: "https://api.digg.se/r2ps-api/v1/".to_string(),
        api_status: "alpha".to_string(),
    };

    (StatusCode::OK, Json(info))
}

/// Health check
///
/// Returns HTTP 200 OK if the service is running and ready to accept requests.
/// This endpoint can be used by load balancers, orchestrators, and monitoring
/// systems to verify that the R2PS service is operational. It performs no
/// dependency checks (Redis, Kafka) — it only confirms the HTTP server is responding.
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is running and ready to accept requests")
    ),
    tag = "Health"
)]
pub async fn health_check() -> impl IntoResponse {
    StatusCode::OK
}
