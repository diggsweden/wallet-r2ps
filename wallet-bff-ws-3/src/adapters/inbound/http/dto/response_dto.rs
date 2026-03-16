use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::hateoas::{Link, Links};

/// Async response status.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AsyncResponseStatus {
    Pending,
    Complete,
    Error,
}

/// Generic async response wrapper with HATEOAS support.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "correlationId": "d290f1ee-6c54-4b01-90e6-d701748f0851",
    "status": "PENDING",
    "resultUrl": "https://api.digg.se/r2ps-api/v1/requests/d290f1ee-6c54-4b01-90e6-d701748f0851",
    "_links": {
        "self": {
            "href": "https://api.digg.se/r2ps-api/v1/requests/d290f1ee-6c54-4b01-90e6-d701748f0851",
            "rel": "self",
            "method": "GET"
        },
        "poll": {
            "href": "https://api.digg.se/r2ps-api/v1/requests/d290f1ee-6c54-4b01-90e6-d701748f0851",
            "rel": "poll",
            "method": "GET"
        }
    }
}))]
pub struct AsyncResponseDto<T> {
    /// Correlation ID for tracking the request
    pub correlation_id: Uuid,

    /// Current status of the async operation
    pub status: AsyncResponseStatus,

    /// Result data (present when status is COMPLETE)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,

    /// URL for polling the result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_url: Option<String>,

    /// Error information (present when status is ERROR)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AsyncResponseError>,

    /// HATEOAS links (REST Level 3)
    #[serde(rename = "_links")]
    pub links: Links,
}

impl<T> AsyncResponseDto<T> {
    pub fn pending(correlation_id: Uuid, result_url: String, base_url: &str) -> Self {
        let links = Links::new_with_self(format!("{}/requests/{}", base_url, correlation_id)).add(
            "poll",
            Link::new(format!("{}/requests/{}", base_url, correlation_id), "GET").with_rel("poll"),
        );

        Self {
            correlation_id,
            status: AsyncResponseStatus::Pending,
            result: None,
            result_url: Some(result_url),
            error: None,
            links,
        }
    }

    pub fn complete(correlation_id: Uuid, result: T, base_url: &str) -> Self {
        let links = Links::new_with_self(format!("{}/requests/{}", base_url, correlation_id));

        Self {
            correlation_id,
            status: AsyncResponseStatus::Complete,
            result: Some(result),
            result_url: None,
            error: None,
            links,
        }
    }

    pub fn error(correlation_id: Uuid, error: AsyncResponseError, base_url: &str) -> Self {
        let links = Links::new_with_self(format!("{}/requests/{}", base_url, correlation_id));

        Self {
            correlation_id,
            status: AsyncResponseStatus::Error,
            result: None,
            result_url: None,
            error: Some(error),
            links,
        }
    }
}

/// Error information in async response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "message": "HSM worker returned an error",
    "httpStatus": 500
}))]
pub struct AsyncResponseError {
    pub message: String,
    pub http_status: u16,
}

/// Response from state initialization.
///
/// Device state is managed server-side by the HSM worker. The response
/// contains the `service_response_jws` which is the encrypted `InnerResponse`
/// from the worker, containing the `dev_authorization_code` and other data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "status": "OK",
    "clientId": "d290f1ee-6c54-4b01-90e6-d701748f0851",
    "serviceResponseJws": "eyJhbGciOiJFUzI1NiJ9...",
    "_links": {
        "self": {
            "href": "https://api.digg.se/r2ps-api/v1/device-states",
            "rel": "self",
            "method": "GET"
        },
        "submit-request": {
            "href": "https://api.digg.se/r2ps-api/v1",
            "rel": "submit-request",
            "method": "POST",
            "type": "application/json"
        }
    }
}))]
pub struct NewStateResponseDto {
    pub status: String,
    pub client_id: String,
    /// The encrypted service response from the HSM worker.
    pub service_response_jws: String,

    /// HATEOAS links
    #[serde(rename = "_links")]
    pub links: Links,
}

impl NewStateResponseDto {
    pub fn new(client_id: String, service_response_jws: String, base_url: &str) -> Self {
        let links = Links::new_with_self(format!("{}/device-states", base_url)).add(
            "submit-request",
            Link::new(base_url.to_string(), "POST")
                .with_rel("submit-request")
                .with_type("application/json"),
        );

        Self {
            status: "OK".to_string(),
            client_id,
            service_response_jws,
            links,
        }
    }
}

/// API Information response (VER.06, VER.07).
/// Required by Swedish REST API Profile v1.2.0.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "apiName": "r2ps-api",
    "apiVersion": "0.1.0",
    "apiReleased": "2024-01-01",
    "apiDocumentation": "https://api.digg.se/r2ps-api/v1/",
    "apiStatus": "alpha"
}))]
pub struct ApiInfoDto {
    /// Name of the API
    pub api_name: String,

    /// Full version number (MAJOR.MINOR.PATCH)
    pub api_version: String,

    /// Release date (RFC 3339 format)
    pub api_released: String,

    /// URL to API documentation
    pub api_documentation: String,

    /// Lifecycle status: alpha, beta, active, deprecated, retired, decommissioned
    pub api_status: String,
}
