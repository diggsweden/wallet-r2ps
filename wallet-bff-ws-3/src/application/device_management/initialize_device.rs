use std::time::Duration;

use crate::domain::{
    device_management::{
        errors::DeviceError,
        value_objects::{ClientId, EcPublicKey},
    },
    hsm_integration::entities::StateInitRequest,
};
use crate::ports::outbound::{ClientKeyRepository, MessagePublisher, StateInitRepository};

/// Result of a successful device initialization.
#[derive(Debug)]
pub struct InitializeDeviceResult {
    pub client_id: ClientId,
    /// The service_response_jws from the HSM worker (contains the encrypted
    /// InnerResponse with dev_authorization_code inside).
    pub service_response_jws: String,
}

/// Use case: Initialize a new device with a public key.
///
/// Orchestrates the flow:
/// 1. Validate & generate client ID
/// 2. Check for existing device (handle overwrite via client key cache)
/// 3. Send state init command to HSM worker (via `r2ps-requests` topic)
/// 4. Wait for HSM response (arrives on `r2ps-responses`, stored by subscriber)
/// 5. Return the service response JWS (device state is managed server-side)
pub struct InitializeDeviceUseCase<K, M, S>
where
    K: ClientKeyRepository,
    M: MessagePublisher,
    S: StateInitRepository,
{
    key_repo: K,
    publisher: M,
    state_init_repo: S,
    init_timeout: Duration,
}

impl<K, M, S> InitializeDeviceUseCase<K, M, S>
where
    K: ClientKeyRepository,
    M: MessagePublisher,
    S: StateInitRepository,
{
    pub fn new(key_repo: K, publisher: M, state_init_repo: S, init_timeout: Duration) -> Self {
        Self {
            key_repo,
            publisher,
            state_init_repo,
            init_timeout,
        }
    }

    pub async fn execute(
        &self,
        public_key: EcPublicKey,
        requested_client_id: Option<String>,
        overwrite: bool,
    ) -> Result<InitializeDeviceResult, DeviceError> {
        // Determine client ID
        let client_id = match requested_client_id {
            Some(id) => ClientId::new(id)?,
            None => ClientId::generate(),
        };

        // Check for existing device (via client key cache)
        if self.key_repo.exists(&client_id).await.unwrap_or(false) && !overwrite {
            return Err(DeviceError::AlreadyExists(client_id.to_string()));
        }

        // Generate correlation ID for the state-init command
        let correlation_id = uuid::Uuid::new_v4().to_string();

        // Build and send the state init request to HSM worker
        // This goes to r2ps-requests with context: "state-init"
        let state_init_request = StateInitRequest::new(
            correlation_id.clone(),
            client_id.as_str().to_string(),
            public_key,
        );

        self.publisher
            .publish_state_init_request(&state_init_request)
            .await
            .map_err(|e| DeviceError::StorageError(e.to_string()))?;

        // Wait for HSM response (stored by KafkaSubscriber in StateInitRepository)
        let response = self
            .state_init_repo
            .wait_for_response(&correlation_id, self.init_timeout)
            .await
            .map_err(|e| DeviceError::StorageError(e.to_string()))?
            .ok_or_else(|| DeviceError::StorageError("timeout waiting for HSM response".into()))?;

        if !response.is_success() {
            return Err(DeviceError::StorageError(format!(
                "HSM worker returned error status {}",
                response.http_status()
            )));
        }

        // Device state is managed server-side by the worker.
        // The state-snapshot consumer will populate the client key cache
        // asynchronously. We return the service response JWS to the caller.

        Ok(InitializeDeviceResult {
            client_id,
            service_response_jws: response.service_response_jws().to_string(),
        })
    }
}
