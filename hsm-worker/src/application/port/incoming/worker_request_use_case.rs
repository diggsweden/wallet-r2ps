// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::domain::HsmWorkerRequest;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Higher-level errors that can occur when processing a worker request.
#[derive(Debug)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum WorkerRequestError {
    /// Failed to connect to a required service
    ConnectionError,
    /// Failed to build a safe error response
    ResponseBuildError,
}

pub trait WorkerRequestUseCase {
    fn execute(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerRequestId, WorkerRequestError>;
}

pub type WorkerRequestId = String;
