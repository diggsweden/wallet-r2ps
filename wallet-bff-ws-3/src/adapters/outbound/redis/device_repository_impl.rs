use redis::AsyncCommands;
use std::time::Duration;

use super::connection::RedisConnection;
use crate::domain::device_management::{
    entities::Device,
    errors::DeviceError,
    value_objects::{ClientId, DeviceState},
};
use crate::ports::outbound::DeviceRepository;

/// Redis-backed implementation of the `DeviceRepository` port.
#[derive(Clone)]
pub struct RedisDeviceRepository {
    conn: RedisConnection,
    ttl: Duration,
}

impl RedisDeviceRepository {
    pub fn new(conn: RedisConnection, ttl: Duration) -> Self {
        Self { conn, ttl }
    }

    fn key(client_id: &ClientId) -> String {
        format!("device:state:{}", client_id)
    }
}

impl DeviceRepository for RedisDeviceRepository {
    async fn save(&self, device: &Device) -> Result<(), DeviceError> {
        let key = Self::key(device.id());
        let value = device.state().as_jws();
        let mut conn = self.conn.get();
        conn.set_ex::<_, _, ()>(&key, value, self.ttl.as_secs())
            .await
            .map_err(|e| DeviceError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn find_by_id(&self, id: &ClientId) -> Result<Option<Device>, DeviceError> {
        let key = Self::key(id);
        let mut conn = self.conn.get();
        let state_jws: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| DeviceError::StorageError(e.to_string()))?;

        match state_jws {
            Some(jws) => {
                let state = DeviceState::new(jws)?;
                Ok(Some(Device::new(id.clone(), state)))
            }
            None => Ok(None),
        }
    }

    async fn exists(&self, id: &ClientId) -> Result<bool, DeviceError> {
        let key = Self::key(id);
        let mut conn = self.conn.get();
        let exists: bool = conn
            .exists(&key)
            .await
            .map_err(|e| DeviceError::StorageError(e.to_string()))?;
        Ok(exists)
    }

    async fn store_state(&self, id: &ClientId, state: &DeviceState) -> Result<(), DeviceError> {
        let key = Self::key(id);
        let mut conn = self.conn.get();
        conn.set_ex::<_, _, ()>(&key, state.as_jws(), self.ttl.as_secs())
            .await
            .map_err(|e| DeviceError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &ClientId) -> Result<(), DeviceError> {
        let key = Self::key(id);
        let mut conn = self.conn.get();
        conn.del::<_, ()>(&key)
            .await
            .map_err(|e| DeviceError::StorageError(e.to_string()))?;
        Ok(())
    }
}
