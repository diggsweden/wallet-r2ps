// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;

use redis::AsyncCommands;
use tracing::error;

use crate::application::port::outgoing::DeviceStatePort;
use crate::infrastructure::adapters::outgoing::redis::{SharedConnection, with_redis_retry};

pub struct DeviceStateRedisAdapter {
    conn: Arc<SharedConnection>,
}

impl DeviceStateRedisAdapter {
    pub fn new(conn: Arc<SharedConnection>) -> Self {
        Self { conn }
    }
}

#[async_trait::async_trait]
impl DeviceStatePort for DeviceStateRedisAdapter {
    async fn save(&self, key: &str, state: &str, ttl_seconds: u64) {
        let result = with_redis_retry(
            &self.conn,
            "device_state.save",
            key,
            |mut conn| async move { conn.set_ex::<_, _, ()>(key, state, ttl_seconds).await },
        )
        .await;
        if let Err(e) = result {
            error!("Failed to save device state for key {}: {}", key, e);
        }
    }

    async fn load(&self, key: &str) -> Result<Option<String>, String> {
        with_redis_retry(
            &self.conn,
            "device_state.load",
            key,
            |mut conn| async move { conn.get::<_, Option<String>>(key).await },
        )
        .await
        .map_err(|e| {
            error!("Failed to load device state for key {}: {}", key, e);
            format!("Device state store error: {e}")
        })
    }
}
