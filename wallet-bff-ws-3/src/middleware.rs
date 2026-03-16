use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Middleware to enforce HTTPS (SAK.01-SAK.03)
/// Rejects HTTP requests in production instead of redirecting
pub async fn require_https(req: Request, next: Next) -> Result<Response, Response> {
    // Check if the request is over HTTPS
    // Priority:
    // 1. X-Forwarded-Proto header (for requests behind reverse proxy)
    // 2. Request URI scheme (for direct connections)
    let scheme = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .or_else(|| req.uri().scheme_str())
        .unwrap_or("http");

    if scheme != "https" {
        tracing::warn!(
            "Rejected non-HTTPS request to {} (scheme: {})",
            req.uri(),
            scheme
        );
        return Err((
            StatusCode::FORBIDDEN,
            "HTTPS required. This API must be accessed over HTTPS (TLS 1.2+) as required by Swedish REST API Profile.",
        )
            .into_response());
    }

    Ok(next.run(req).await)
}
