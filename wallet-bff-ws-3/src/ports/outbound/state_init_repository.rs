use crate::domain::hsm_integration::{entities::StateInitResponse, errors::HsmError};
use std::time::Duration;

/// Port for storing and retrieving state initialization responses.
///
/// State-init responses now arrive as regular worker responses on `r2ps-responses`.
/// This repository provides ephemeral storage for the synchronous wait pattern:
/// the HTTP handler publishes a state-init command and polls this repository
/// until the response arrives (or times out).
pub trait StateInitRepository: Send + Sync {
    /// Store a state init response keyed by correlation ID.
    fn store_response(
        &self,
        response: &StateInitResponse,
    ) -> impl Future<Output = Result<(), HsmError>> + Send;

    /// Retrieve a state init response by correlation ID.
    fn get_response(
        &self,
        correlation_id: &str,
    ) -> impl Future<Output = Result<Option<StateInitResponse>, HsmError>> + Send;

    /// Wait for a state init response, polling with the given timeout.
    fn wait_for_response(
        &self,
        correlation_id: &str,
        timeout: Duration,
    ) -> impl Future<Output = Result<Option<StateInitResponse>, HsmError>> + Send;
}
