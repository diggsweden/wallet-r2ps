// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use tracing::error;

use crate::application::port::outgoing::RequestContextPort;
use crate::domain::RequestContext;

const KEY_PREFIX: &str = "req-ctx:";

pub struct RequestContextRedisAdapter {
    conn: ConnectionManager,
}

impl RequestContextRedisAdapter {
    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    fn key(request_id: &str) -> String {
        format!("{KEY_PREFIX}{request_id}")
    }
}

#[async_trait::async_trait]
impl RequestContextPort for RequestContextRedisAdapter {
    async fn store(
        &self,
        request_id: &str,
        ctx: &RequestContext,
        ttl_seconds: u64,
    ) -> Result<(), String> {
        let mut conn = self.conn.clone();
        let bytes = serde_json::to_vec(ctx)
            .map_err(|e| format!("Failed to serialize RequestContext: {e}"))?;
        conn.set_ex::<_, _, ()>(Self::key(request_id), bytes, ttl_seconds)
            .await
            .map_err(|e| {
                error!("Failed to store request context {}: {}", request_id, e);
                format!("Redis SET error: {e}")
            })
    }

    async fn take(&self, request_id: &str) -> Option<RequestContext> {
        let mut conn = self.conn.clone();
        let key = Self::key(request_id);

        // GETDEL is atomic: return-and-remove. Falls back to GET if the server
        // is older than Redis 6.2, but our deployment requires Redis ≥ 7.
        let bytes: Option<Vec<u8>> = match redis::cmd("GETDEL")
            .arg(&key)
            .query_async(&mut conn)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to GETDEL request context {}: {}", request_id, e);
                return None;
            }
        };

        bytes.and_then(|b| match serde_json::from_slice(&b) {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                error!("Failed to deserialize request context {}: {}", request_id, e);
                None
            }
        })
    }
}
