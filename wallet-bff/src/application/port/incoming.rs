// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::domain::{HsmWorkerResponse, StateInitResponse};

/// Use case port: ingest an hsm-worker response from Kafka, save any updated
/// device state, and publish the response envelope to the shared store so a
/// polling client can pick it up.
#[async_trait::async_trait]
pub trait HsmResponseSinkPort: Send + Sync {
    async fn ingest(&self, response: HsmWorkerResponse);
}

/// Use case port: ingest a state-init response from Kafka, save the device
/// state, and publish the response envelope to the shared store.
#[async_trait::async_trait]
pub trait StateInitResponseSinkPort: Send + Sync {
    async fn ingest(&self, response: StateInitResponse);
}
