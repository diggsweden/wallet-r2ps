// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use serde::Deserialize;
use std::sync::Arc;
use tracing::warn;

use crate::application::port::outgoing::NoncePort;

pub struct ReplayProtectionState {
    pub nonce_port: Arc<dyn NoncePort>,
    pub nonce_ttl_seconds: u64,
}

/// Minimal structs for extracting the nonce from the request body without
/// deserializing the full types.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayBody {
    client_id: Option<String>,
    outer_request_jws: Option<String>,
}

#[derive(Deserialize)]
struct NonceOnly {
    nonce: String,
}

pub async fn replay_protection(
    State(rp): State<Arc<ReplayProtectionState>>,
    request: Request,
    next: Next,
) -> Response {
    // Skip GET requests (polling endpoint has no replay risk)
    if request.method() == axum::http::Method::GET {
        return next.run(request).await;
    }

    let instance = request.uri().path().to_string();

    // Skip state-init: no outer_request_jws in body
    if instance == "/hsm/v1/device-states" {
        return next.run(request).await;
    }

    // Read body bytes so we can extract the nonce and then pass the body on
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            warn!("Failed to read request body for nonce extraction: {}", e);
            return super::problem_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some("Failed to read request body."),
                &instance,
            );
        }
    };

    // Extract client_id and outer_request_jws from the JSON body
    let (client_id, jws_str) = match serde_json::from_slice::<ReplayBody>(&bytes) {
        Ok(b) => match (b.client_id, b.outer_request_jws) {
            (Some(cid), Some(jws)) => (cid, jws),
            _ => {
                // Missing fields — let the handler produce the proper error response
                let request = Request::from_parts(parts, Body::from(bytes));
                return next.run(request).await;
            }
        },
        Err(_) => {
            // Body is not valid JSON — let the handler produce the proper error response
            let request = Request::from_parts(parts, Body::from(bytes));
            return next.run(request).await;
        }
    };

    // Decode the JWS payload without verifying the signature (verification is the worker's job)
    let payload_bytes = match hsm_common::jose::jws_decode_unverified(&jws_str) {
        Ok(b) => b,
        Err(_) => {
            warn!("Malformed JWS in outerRequestJws on {}", instance);
            return super::problem_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some("The 'outerRequestJws' field is not a valid compact JWS."),
                &instance,
            );
        }
    };

    // Extract nonce from the decoded payload
    let nonce = match serde_json::from_slice::<NonceOnly>(&payload_bytes) {
        Ok(n) => n.nonce,
        Err(_) => {
            warn!(
                "Missing or invalid nonce in outerRequestJws payload on {}",
                instance
            );
            return super::problem_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some("The JWS payload must contain a 'nonce' field."),
                &instance,
            );
        }
    };

    // Check/store nonce in Valkey, namespaced by client_id
    match rp
        .nonce_port
        .try_store(&client_id, &nonce, rp.nonce_ttl_seconds)
        .await
    {
        Ok(true) => {} // New nonce, proceed
        Ok(false) => {
            warn!("Duplicate nonce detected: {}", nonce);
            return super::problem_response(
                StatusCode::CONFLICT,
                "Duplicate Request",
                Some("This nonce has already been used. Each request must have a unique nonce."),
                &instance,
            );
        }
        Err(e) => {
            tracing::error!("Nonce store error: {}", e);
            return super::service_unavailable_problem(
                &instance,
                "Nonce store is temporarily unavailable. Please retry.",
            );
        }
    }

    // Reconstruct the request with the body and forward it
    let request = Request::from_parts(parts, Body::from(bytes));
    next.run(request).await
}
