use crate::domain::device_management::value_objects::{ClientId, EcPublicKeyData};

/// Port for looking up client public keys for WebSocket authentication.
///
/// Implementations cache the device public keys extracted from the
/// `state-snapshot` Kafka topic. Keys are stored per `client_id` and
/// identified by `kid`.
pub trait ClientKeyRepository: Send + Sync {
    /// Store the full set of public keys for a client.
    ///
    /// Replaces any previously stored keys for this `client_id`.
    fn store_keys(
        &self,
        client_id: &ClientId,
        keys: &[EcPublicKeyData],
    ) -> impl Future<Output = Result<(), ClientKeyError>> + Send;

    /// Look up a specific public key by `client_id` and `kid`.
    fn find_key(
        &self,
        client_id: &ClientId,
        kid: &str,
    ) -> impl Future<Output = Result<Option<EcPublicKeyData>, ClientKeyError>> + Send;

    /// Return all public keys for a given `client_id`.
    fn find_all_keys(
        &self,
        client_id: &ClientId,
    ) -> impl Future<Output = Result<Vec<EcPublicKeyData>, ClientKeyError>> + Send;

    /// Check whether any keys exist for a `client_id`.
    fn exists(
        &self,
        client_id: &ClientId,
    ) -> impl Future<Output = Result<bool, ClientKeyError>> + Send;

    /// Remove all keys for a `client_id`.
    fn delete(
        &self,
        client_id: &ClientId,
    ) -> impl Future<Output = Result<(), ClientKeyError>> + Send;
}

/// Errors from the client key repository.
#[derive(Debug, thiserror::Error)]
pub enum ClientKeyError {
    #[error("storage error: {0}")]
    StorageError(String),

    #[error("serialization error: {0}")]
    SerializationError(String),
}
