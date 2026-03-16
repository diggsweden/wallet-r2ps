use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use super::connection::RedisConnection;
use crate::domain::device_management::value_objects::{ClientId, EcPublicKeyData};
use crate::ports::outbound::{ClientKeyError, ClientKeyRepository};

/// Serializable wrapper for storing EC public key data in Valkey/Redis.
#[derive(Debug, Serialize, Deserialize)]
struct StoredKeyEntry {
    kty: String,
    crv: String,
    x: String,
    y: String,
    kid: String,
}

impl From<&EcPublicKeyData> for StoredKeyEntry {
    fn from(key: &EcPublicKeyData) -> Self {
        Self {
            kty: key.kty.clone(),
            crv: key.crv.clone(),
            x: key.x.clone(),
            y: key.y.clone(),
            kid: key.kid.clone(),
        }
    }
}

impl From<StoredKeyEntry> for EcPublicKeyData {
    fn from(entry: StoredKeyEntry) -> Self {
        Self {
            kty: entry.kty,
            crv: entry.crv,
            x: entry.x,
            y: entry.y,
            kid: entry.kid,
        }
    }
}

/// Valkey/Redis-backed implementation of the `ClientKeyRepository` port.
///
/// Stores client public keys using a Redis hash per `client_id`:
///   Key: `client:keys:{client_id}`
///   Hash fields: `{kid}` → JSON-serialized `EcPublicKeyData`
///
/// Populated by the state-snapshot Kafka consumer. Used by WebSocket
/// auth to look up client public keys by `client_id + kid`.
#[derive(Clone)]
pub struct RedisClientKeyRepository {
    conn: RedisConnection,
}

impl RedisClientKeyRepository {
    pub fn new(conn: RedisConnection) -> Self {
        Self { conn }
    }

    fn key(client_id: &ClientId) -> String {
        format!("client:keys:{}", client_id)
    }
}

impl ClientKeyRepository for RedisClientKeyRepository {
    async fn store_keys(
        &self,
        client_id: &ClientId,
        keys: &[EcPublicKeyData],
    ) -> Result<(), ClientKeyError> {
        let key = Self::key(client_id);
        let mut conn = self.conn.get();

        // Delete existing keys first, then set new ones atomically
        conn.del::<_, ()>(&key)
            .await
            .map_err(|e| ClientKeyError::StorageError(e.to_string()))?;

        if keys.is_empty() {
            return Ok(());
        }

        // Store each key entry as a hash field keyed by kid
        for key_entry in keys {
            let stored = StoredKeyEntry::from(key_entry);
            let json = serde_json::to_string(&stored)
                .map_err(|e| ClientKeyError::SerializationError(e.to_string()))?;
            conn.hset::<_, _, _, ()>(&key, &key_entry.kid, json)
                .await
                .map_err(|e| ClientKeyError::StorageError(e.to_string()))?;
        }

        Ok(())
    }

    async fn find_key(
        &self,
        client_id: &ClientId,
        kid: &str,
    ) -> Result<Option<EcPublicKeyData>, ClientKeyError> {
        let key = Self::key(client_id);
        let mut conn = self.conn.get();

        let json: Option<String> = conn
            .hget(&key, kid)
            .await
            .map_err(|e| ClientKeyError::StorageError(e.to_string()))?;

        match json {
            Some(j) => {
                let stored: StoredKeyEntry = serde_json::from_str(&j)
                    .map_err(|e| ClientKeyError::SerializationError(e.to_string()))?;
                Ok(Some(stored.into()))
            }
            None => Ok(None),
        }
    }

    async fn find_all_keys(
        &self,
        client_id: &ClientId,
    ) -> Result<Vec<EcPublicKeyData>, ClientKeyError> {
        let key = Self::key(client_id);
        let mut conn = self.conn.get();

        let entries: std::collections::HashMap<String, String> = conn
            .hgetall(&key)
            .await
            .map_err(|e| ClientKeyError::StorageError(e.to_string()))?;

        let mut keys = Vec::with_capacity(entries.len());
        for (_kid, json) in entries {
            let stored: StoredKeyEntry = serde_json::from_str(&json)
                .map_err(|e| ClientKeyError::SerializationError(e.to_string()))?;
            keys.push(stored.into());
        }

        Ok(keys)
    }

    async fn exists(&self, client_id: &ClientId) -> Result<bool, ClientKeyError> {
        let key = Self::key(client_id);
        let mut conn = self.conn.get();
        let exists: bool = conn
            .exists(&key)
            .await
            .map_err(|e| ClientKeyError::StorageError(e.to_string()))?;
        Ok(exists)
    }

    async fn delete(&self, client_id: &ClientId) -> Result<(), ClientKeyError> {
        let key = Self::key(client_id);
        let mut conn = self.conn.get();
        conn.del::<_, ()>(&key)
            .await
            .map_err(|e| ClientKeyError::StorageError(e.to_string()))?;
        Ok(())
    }
}
