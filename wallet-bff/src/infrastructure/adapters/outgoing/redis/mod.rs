// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

pub mod device_state;
pub mod nonce;

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::sentinel::{SentinelClientBuilder, SentinelServerType};
use redis::{Client, ConnectionAddr, ErrorKind, IntoConnectionInfo, ServerErrorKind};
use tokio::sync::Mutex;
use tracing::{Instrument, info, warn};

use crate::infrastructure::config::AppConfig;

/// Holds a hot-swappable [`ConnectionManager`] for the current Valkey primary.
/// The manager itself carries bounded reconnect and per-command timeouts; this
/// wrapper lets callers *replace* the manager when Sentinel promotes a new
/// master and the old address either becomes a read-only replica or vanishes.
pub struct SharedConnection {
    inner: ArcSwap<ConnectionManager>,
    rebuild_lock: Mutex<()>,
    // None disables re-resolve (test-only construction path). In production
    // this is always Some, so a failover triggers a fresh Sentinel lookup.
    config: Option<Arc<AppConfig>>,
}

impl SharedConnection {
    /// Resolves the current master (via Sentinel when configured) and builds
    /// the initial ConnectionManager. Panics on failure — only called at
    /// startup, where a broken Redis makes the process unusable anyway.
    pub async fn connect(config: Arc<AppConfig>) -> Self {
        let cm = build_manager(&config)
            .await
            .expect("Failed to connect to Redis");
        Self {
            inner: ArcSwap::from(Arc::new(cm)),
            rebuild_lock: Mutex::new(()),
            config: Some(config),
        }
    }

    /// Test-only: wrap an existing manager. Re-resolve is a no-op (no
    /// Sentinel config is available), which is fine because tests point at
    /// a single non-HA Valkey container.
    #[doc(hidden)]
    pub fn from_manager(cm: ConnectionManager) -> Self {
        Self {
            inner: ArcSwap::from(Arc::new(cm)),
            rebuild_lock: Mutex::new(()),
            config: None,
        }
    }

    /// Returns the currently-active ConnectionManager. Cloning it is cheap —
    /// ConnectionManager internally shares its socket and reconnect state.
    pub fn current(&self) -> ConnectionManager {
        (**self.inner.load()).clone()
    }

    /// Re-resolves the master via Sentinel and atomically swaps in a fresh
    /// ConnectionManager. Concurrent callers are serialized so we only do
    /// one Sentinel round-trip per failover event.
    async fn reresolve(&self) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let _guard = self.rebuild_lock.lock().await;
        match build_manager(config).await {
            Ok(cm) => {
                self.inner.store(Arc::new(cm));
                info!("Redis primary re-resolved via Sentinel");
            }
            Err(e) => {
                warn!(error = %e, "Sentinel re-resolve failed; keeping previous connection");
            }
        }
    }
}

/// Retries `f` on connection-class and failover errors. Up to six attempts,
/// with exponential backoff between plain retries (50, 100, 200, 400, 800 ms,
/// capped at 1 s). The wider budget absorbs sub-second connection tear-downs
/// driven by service-mesh sidecars (Envoy drain / `max_connection_duration`)
/// so a single Redis call reconnects transparently instead of surfacing.
///
///  * `READONLY` (from a replica demoted by Sentinel failover) or `MASTERDOWN`
///    → re-resolve via Sentinel immediately, brief pause, then keep retrying.
///  * IO / timeout / connection-dropped → plain retry against the current
///    manager (transient blip); if still failing, re-resolve once — the
///    address itself may be stale after a failover — and keep retrying.
///
/// Emits a `redis.cmd` span with `attempts` and (when triggered) `reresolved`.
pub(super) async fn with_redis_retry<T, F, Fut>(
    shared: &SharedConnection,
    op: &'static str,
    key: &str,
    mut f: F,
) -> redis::RedisResult<T>
where
    F: FnMut(ConnectionManager) -> Fut,
    Fut: std::future::Future<Output = redis::RedisResult<T>>,
{
    const MAX_ATTEMPTS: u32 = 6;
    let span = tracing::info_span!(
        "redis.cmd",
        op,
        key,
        attempts = tracing::field::Empty,
        reresolved = tracing::field::Empty,
    );
    async move {
        let mut attempts: u32 = 0;
        let mut reresolved = false;
        loop {
            attempts += 1;
            let conn = shared.current();
            match f(conn).await {
                Ok(v) => {
                    tracing::Span::current().record("attempts", attempts);
                    if reresolved {
                        tracing::Span::current().record("reresolved", true);
                    }
                    return Ok(v);
                }
                Err(e) if attempts < MAX_ATTEMPTS => {
                    let failover = is_failover_error(&e);
                    let transient = is_retryable_io(&e);
                    if !failover && !transient {
                        tracing::Span::current().record("attempts", attempts);
                        return Err(e);
                    }

                    // Re-resolve on the first failover signal we see, or after
                    // a plain retry has already failed with a transient error
                    // (the old master's address may itself be stale).
                    let should_reresolve = !reresolved && (failover || attempts >= 2);
                    if should_reresolve {
                        warn!(op, key, attempt = attempts, error = %e,
                              "re-resolving Redis master via Sentinel");
                        shared.reresolve().await;
                        reresolved = true;
                        // Give the newly-promoted master a moment to accept writes.
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    } else {
                        warn!(op, key, attempt = attempts, error = %e, "redis retry");
                        let shift = (attempts - 1).min(5);
                        let sleep_ms = (50u64 << shift).min(1000);
                        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                    }
                }
                Err(e) => {
                    tracing::Span::current().record("attempts", attempts);
                    if reresolved {
                        tracing::Span::current().record("reresolved", true);
                    }
                    return Err(e);
                }
            }
        }
    }
    .instrument(span)
    .await
}

fn is_failover_error(e: &redis::RedisError) -> bool {
    matches!(
        e.kind(),
        ErrorKind::Server(ServerErrorKind::ReadOnly)
            | ErrorKind::Server(ServerErrorKind::MasterDown)
    )
}

fn is_retryable_io(e: &redis::RedisError) -> bool {
    !e.is_unrecoverable_error() && (e.is_io_error() || e.is_connection_dropped() || e.is_timeout())
}

async fn build_manager(config: &AppConfig) -> redis::RedisResult<ConnectionManager> {
    let client = resolve_master_client(config).await?;
    let cm_cfg = ConnectionManagerConfig::new()
        .set_number_of_retries(3)
        .set_max_delay(Duration::from_secs(2))
        .set_connection_timeout(Some(Duration::from_millis(500)))
        .set_response_timeout(Some(Duration::from_secs(2)));
    ConnectionManager::new_with_config(client, cm_cfg).await
}

/// TCP keepalive + (Linux) TCP_USER_TIMEOUT for the Valkey client socket.
/// The probe schedule (30s idle, 10s interval, 3 retries) keeps the connection
/// "active" so Istio's sidecar idle timer cannot kill it, and detects a dead
/// peer within ~60s. TCP_USER_TIMEOUT bounds how long an in-flight write hangs
/// on a half-closed socket before erroring.
fn redis_tcp_settings() -> redis::io::tcp::TcpSettings {
    use redis::io::tcp::TcpSettings;
    use redis::io::tcp::socket2::TcpKeepalive;
    let ka = TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10))
        .with_retries(3);
    let s = TcpSettings::default().set_keepalive(ka);
    #[cfg(target_os = "linux")]
    let s = s.set_user_timeout(Duration::from_secs(20));
    s
}

/// Returns a [`Client`] pointing at the current Redis primary. If
/// `REDIS_SENTINEL_HOSTS` + `REDIS_SENTINEL_MASTER` are set the master is
/// resolved via Sentinel (Bitnami valkey HA topology); otherwise the
/// connection is direct.
async fn resolve_master_client(config: &AppConfig) -> redis::RedisResult<Client> {
    let hosts = config.redis_sentinel_hosts.trim();
    let master = config.redis_sentinel_master.trim();
    if hosts.is_empty() || master.is_empty() {
        let info = config
            .redis_url()
            .into_connection_info()?
            .set_tcp_settings(redis_tcp_settings());
        return Client::open(info);
    }

    let addrs: Vec<ConnectionAddr> = hosts
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|hp| {
            let (h, p) = hp
                .rsplit_once(':')
                .expect("REDIS_SENTINEL_HOSTS entries must be host:port");
            ConnectionAddr::Tcp(h.to_string(), p.parse().expect("invalid sentinel port"))
        })
        .collect();

    let mut sc = SentinelClientBuilder::new(addrs, master, SentinelServerType::Master)?
        .set_client_to_redis_db(i64::from(config.redis_database))
        .set_client_to_redis_username(config.redis_username.clone())
        .set_client_to_redis_password(config.redis_password.clone())
        .set_client_to_redis_tcp_settings(redis_tcp_settings())
        .set_client_to_sentinel_tcp_settings(redis_tcp_settings())
        .build()?;

    info!(
        "Resolving Redis primary via Sentinel (hosts={}, master={})",
        hosts, master
    );
    sc.async_get_client().await
}
