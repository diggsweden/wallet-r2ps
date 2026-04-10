// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::domain::HsmWorkerResponse;

/// Use case port for handling incoming worker responses.
#[async_trait::async_trait]
pub trait ResponseUseCase: Send + Sync {
    async fn response_ready(&self, response: HsmWorkerResponse);
    async fn wait_for_response(
        &self,
        request_id: &str,
        timeout_ms: u64,
    ) -> Option<crate::domain::CachedResponse>;
}
