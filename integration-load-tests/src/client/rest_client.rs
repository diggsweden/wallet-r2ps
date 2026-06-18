// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! HTTP REST client for the BFF.
//!
//! Both endpoints are fully async:
//!   POST /hsm/v1/device-states      -> 202 Accepted + Location header
//!   POST /hsm/v1/requests           -> 202 Accepted + Location header
//!   GET  /hsm/v1/device-states/{id} -> 200 OK (long-poll completed)
//!                                      | 202 Accepted (still pending)
//!   GET  /hsm/v1/requests/{id}      -> 200 OK | 202 Accepted
//!
//! The client always follows the `Location` URL returned by the POST and
//! long-polls until the BFF returns a 200.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::time::Duration;

use crate::backend::BackendClient;
use crate::protocol::types::{
    BffNewStateRequest, BffNewStateResponse, BffRequest, BffSyncResponse,
};

/// Maximum wall-clock time spent waiting for an async response across all
/// re-poll round-trips. Each individual GET is long-polled server-side.
const POLL_DEADLINE: Duration = Duration::from_secs(60);

pub struct RestClient {
    client: Client,
    base_url: String,
}

impl RestClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// Long-poll a `/hsm/v1/requests/{id}` URL until the BFF returns 200.
    async fn poll_request(&self, url: &str) -> Result<BffSyncResponse> {
        let deadline = tokio::time::Instant::now() + POLL_DEADLINE;
        loop {
            let resp = self
                .client
                .get(url)
                .send()
                .await
                .context("GET poll request failed")?;

            let status = resp.status();
            if status == StatusCode::OK {
                return resp
                    .json::<BffSyncResponse>()
                    .await
                    .context("Failed to parse poll response");
            }
            if status == StatusCode::ACCEPTED {
                let _ = resp.text().await;
                if tokio::time::Instant::now() >= deadline {
                    bail!("Polling exhausted for {}", url);
                }
                continue;
            }

            let text = resp.text().await.unwrap_or_default();
            bail!("GET {} failed: {} -- {}", url, status, text);
        }
    }

    /// Long-poll a `/hsm/v1/device-states/{id}` URL until the BFF returns 200.
    async fn poll_state_init(&self, url: &str) -> Result<BffNewStateResponse> {
        let deadline = tokio::time::Instant::now() + POLL_DEADLINE;
        loop {
            let resp = self
                .client
                .get(url)
                .send()
                .await
                .context("GET state-init poll failed")?;

            let status = resp.status();
            if status == StatusCode::OK {
                return resp
                    .json::<BffNewStateResponse>()
                    .await
                    .context("Failed to parse state-init poll response");
            }
            if status == StatusCode::ACCEPTED {
                let _ = resp.text().await;
                if tokio::time::Instant::now() >= deadline {
                    bail!("State-init polling exhausted for {}", url);
                }
                continue;
            }

            let text = resp.text().await.unwrap_or_default();
            bail!("GET {} failed: {} -- {}", url, status, text);
        }
    }
}

#[async_trait]
impl BackendClient for RestClient {
    /// POST /hsm/v1/device-states then long-poll the returned Location until
    /// the BFF reports the state-init result.
    async fn create_device_state(
        &self,
        request: &BffNewStateRequest,
    ) -> Result<BffNewStateResponse> {
        let url = format!("{}/hsm/v1/device-states", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .context("POST /device-states failed")?;

        let location = expect_accepted_with_location(resp, "POST /device-states").await?;
        self.poll_state_init(&location).await
    }

    /// POST /hsm/v1/requests then long-poll the returned Location until the
    /// BFF reports the worker result. Retries on 404 to handle the race where
    /// a state-init poll completed before the device-state Redis snapshot was
    /// visible across replicas.
    async fn submit_request(
        &self,
        client_id: &str,
        outer_request_jws: &str,
    ) -> Result<BffSyncResponse> {
        let url = format!("{}/hsm/v1/requests", self.base_url);
        let body = BffRequest {
            client_id: client_id.to_string(),
            outer_request_jws: outer_request_jws.to_string(),
        };

        let max_retries = 5;
        let base_delay = Duration::from_millis(200);

        for attempt in 0..=max_retries {
            let resp = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await
                .context("POST /requests failed")?;

            if resp.status() == StatusCode::ACCEPTED {
                let location = take_location(&resp, "POST /requests")?;
                let _ = resp.text().await;
                return self.poll_request(&location).await;
            }

            if resp.status().as_u16() == 404 && attempt < max_retries {
                let _ = resp.text().await;
                tokio::time::sleep(base_delay * (attempt as u32 + 1)).await;
                continue;
            }

            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("POST /requests failed: {} -- {}", status, text);
        }

        bail!("POST /hsm/v1/requests exhausted retries");
    }
}

/// Drains the response body and returns the `Location` header for a 202
/// Accepted response. Bails for any other status.
async fn expect_accepted_with_location(
    resp: reqwest::Response,
    context: &str,
) -> Result<String> {
    let status = resp.status();
    if status != StatusCode::ACCEPTED {
        let body = resp.text().await.unwrap_or_default();
        bail!("{} expected 202, got: {} -- {}", context, status, body);
    }
    let location = take_location(&resp, context)?;
    let _ = resp.text().await;
    Ok(location)
}

fn take_location(resp: &reqwest::Response, context: &str) -> Result<String> {
    resp.headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .with_context(|| format!("{context} 202 response missing Location header"))
}
