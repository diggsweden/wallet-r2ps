// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use tracing::error;

use crate::application::port::outgoing::NoncePort;
use crate::infrastructure::adapters::outgoing::redis::conn::SharedConn;

const KEY_PREFIX: &str = "nonce:";

pub struct NonceRedisAdapter {
    conn: SharedConn,
}

impl NonceRedisAdapter {
    pub fn new(conn: SharedConn) -> Self {
        Self { conn }
    }
}

#[async_trait::async_trait]
impl NoncePort for NonceRedisAdapter {
    async fn try_store(
        &self,
        client_id: &str,
        nonce: &str,
        ttl_seconds: u64,
    ) -> Result<bool, String> {
        let mut conn = self.conn.read().await.clone();
        let key = format!("{}{}:{}", KEY_PREFIX, client_id, nonce);

        // SET key 1 NX EX ttl — atomic: only sets if key does not exist.
        let result: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg(1)
            .arg("NX")
            .arg("EX")
            .arg(ttl_seconds)
            .query_async(&mut conn)
            .await
            .map_err(|e| {
                error!("Failed to store nonce {}: {}", key, e);
                format!("Nonce store error: {e}")
            })?;

        // Redis returns "OK" when the key was set, None when it already existed.
        Ok(result.is_some())
    }
}
