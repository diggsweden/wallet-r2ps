// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod device_state;
pub mod nonce;

use std::time::Duration;
use tracing::Instrument;

/// Retries `f` on connection-class redis errors (Istio idle-kill, TCP RST,
/// half-open sockets, response timeouts). Up to two retries with 50/100ms
/// backoff. Emits a `redis.cmd` span; the final attempt count is recorded
/// on the span and visible in OTLP traces.
pub(super) async fn with_redis_retry<T, F, Fut>(
    op: &'static str,
    key: &str,
    mut f: F,
) -> redis::RedisResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = redis::RedisResult<T>>,
{
    let span = tracing::info_span!("redis.cmd", op, key, attempts = tracing::field::Empty);
    async move {
        let mut attempts: u32 = 0;
        loop {
            attempts += 1;
            match f().await {
                Ok(v) => {
                    tracing::Span::current().record("attempts", attempts);
                    return Ok(v);
                }
                Err(e) if attempts <= 2 && retryable(&e) => {
                    tracing::warn!(op, key, attempt = attempts, error = %e, "redis retry");
                    tokio::time::sleep(Duration::from_millis(50 * u64::from(attempts))).await;
                }
                Err(e) => {
                    tracing::Span::current().record("attempts", attempts);
                    return Err(e);
                }
            }
        }
    }
    .instrument(span)
    .await
}

fn retryable(e: &redis::RedisError) -> bool {
    !e.is_unrecoverable_error() && (e.is_io_error() || e.is_connection_dropped() || e.is_timeout())
}
