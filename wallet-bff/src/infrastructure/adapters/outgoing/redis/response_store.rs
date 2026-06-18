// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Duration;

use futures_util::StreamExt;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use tracing::error;

use crate::application::port::outgoing::ResponseStorePort;

/// Redis-backed response store.
///
/// Layout per response:
///   - `{key}`        — JSON-encoded envelope, `SET ... EX ttl`. Idempotent
///     across multiple polls until the TTL elapses.
///   - `{key}:notify` — Pub/Sub channel a poller subscribes to so it wakes the
///     instant a response is published, instead of burning CPU on a tight
///     poll loop.
///
/// Pub/Sub is used in preference to BLPOP because BLPOP holds the underlying
/// connection on the server side and would serialise every concurrent poll
/// through the connection pool. A Pub/Sub subscription, by contrast, runs on
/// a dedicated subscriber connection per long-poll, so SET/GET traffic on the
/// shared ConnectionManager is never blocked behind a long-poll.
pub struct ResponseStoreRedisAdapter {
    /// Shared multiplexed connection used for SET/GET. PUBLISH also goes here.
    conn: ConnectionManager,
    /// Client used to allocate a fresh PubSub connection per `await_value`
    /// call. May be `None` in test setups that only exercise SET/GET — those
    /// callers must not invoke `await_value`.
    client: Option<redis::Client>,
}

impl ResponseStoreRedisAdapter {
    /// SET/GET/PUBLISH only. Suitable for the Kafka response consumer side,
    /// which only writes.
    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn, client: None }
    }

    /// SET/GET/PUBLISH plus dedicated Pub/Sub subscriptions on each call to
    /// [`ResponseStorePort::await_value`]. Used by the HTTP handlers.
    pub fn with_pubsub(conn: ConnectionManager, client: redis::Client) -> Self {
        Self {
            conn,
            client: Some(client),
        }
    }

    fn notify_channel(key: &str) -> String {
        format!("{key}:notify")
    }
}

#[async_trait::async_trait]
impl ResponseStorePort for ResponseStoreRedisAdapter {
    async fn put(&self, key: &str, value: &[u8], ttl_seconds: u64) -> Result<(), String> {
        let mut conn = self.conn.clone();
        let channel = Self::notify_channel(key);

        // SET first so any subscriber that wakes from PUBLISH can read it.
        conn.set_ex::<_, _, ()>(key, value, ttl_seconds)
            .await
            .map_err(|e| {
                error!("Failed to SET response key {}: {}", key, e);
                format!("Redis SET error: {e}")
            })?;

        // PUBLISH is fire-and-forget: it returns the number of subscribers
        // (zero is fine; the next poller will see the value via GET).
        let _: i64 = conn
            .publish(&channel, b"1".as_ref())
            .await
            .map_err(|e| format!("Redis PUBLISH error: {e}"))?;

        Ok(())
    }

    async fn await_value(
        &self,
        key: &str,
        timeout_seconds: u64,
    ) -> Result<Option<Vec<u8>>, String> {
        let mut conn = self.conn.clone();

        // Fast path.
        let existing: Option<Vec<u8>> = conn
            .get(key)
            .await
            .map_err(|e| format!("Redis GET error: {e}"))?;
        if existing.is_some() {
            return Ok(existing);
        }

        let Some(client) = self.client.as_ref() else {
            // Pub/Sub disabled (writer-only adapter); behave as a non-blocking
            // GET. Callers in test code use the writer-only constructor.
            return Ok(None);
        };

        // Subscribe BEFORE the second GET to close the race window: if the
        // producer's PUBLISH lands after our first GET but before subscribe(),
        // it would otherwise be lost.
        let mut pubsub = client
            .get_async_pubsub()
            .await
            .map_err(|e| format!("Redis pubsub open error: {e}"))?;
        let channel = Self::notify_channel(key);
        pubsub
            .subscribe(&channel)
            .await
            .map_err(|e| format!("Redis SUBSCRIBE error: {e}"))?;

        // Re-check now that we're subscribed: the producer may have written
        // and published between the first GET and the SUBSCRIBE above.
        let after_sub: Option<Vec<u8>> = conn
            .get(key)
            .await
            .map_err(|e| format!("Redis GET error: {e}"))?;
        if after_sub.is_some() {
            return Ok(after_sub);
        }

        // Wait up to timeout_seconds for the first message — the message body
        // is ignored; the next GET is the source of truth.
        let mut stream = pubsub.on_message();
        let _ = tokio::time::timeout(Duration::from_secs(timeout_seconds), stream.next()).await;

        let value: Option<Vec<u8>> = conn
            .get(key)
            .await
            .map_err(|e| format!("Redis GET error: {e}"))?;
        Ok(value)
    }
}
