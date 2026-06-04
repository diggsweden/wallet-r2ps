// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Connection bootstrap for the Redis/Valkey adapters.
//!
//! Two paths:
//! - Direct: build a `ConnectionManager` from `redis://...` (legacy
//!   single-node, used for docker-compose and local tests).
//! - Sentinel: ask `SentinelClient` for the current master, wrap the
//!   returned `Client` in a `ConnectionManager`. A background watcher
//!   polls Sentinel for the master and swaps the manager when failover
//!   moves the master to a different pod; the rest of the BFF talks to
//!   a single `SharedConn` and never needs to know which path it is on.
//!
//! Adapters clone the inner manager per call. `ConnectionManager` is
//! internally Arc-backed, so cloning is cheap.

use std::sync::Arc;
use std::time::Duration;

use redis::aio::ConnectionManager;
use redis::sentinel::{SentinelClient, SentinelNodeConnectionInfo, SentinelServerType};
use redis::{Client, ConnectionAddr, ConnectionInfo, RedisConnectionInfo};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use crate::infrastructure::config::AppConfig;

pub type SharedConn = Arc<RwLock<ConnectionManager>>;

pub async fn build(config: &AppConfig) -> SharedConn {
    let sentinel_hosts = config.redis_sentinel_hosts.trim();
    if sentinel_hosts.is_empty() {
        let client =
            Client::open(config.redis_url()).expect("Failed to create direct Redis client");
        let mgr = ConnectionManager::new(client)
            .await
            .expect("Failed to connect to Redis (direct)");
        return Arc::new(RwLock::new(mgr));
    }

    let state = SentinelState::new(config).expect("Failed to build SentinelClient");
    let initial = state
        .build_master_manager()
        .await
        .expect("Failed to discover Redis master via Sentinel");
    let shared = Arc::new(RwLock::new(initial.manager));

    let watcher = MasterWatcher {
        sentinel: state,
        shared: shared.clone(),
        last_master: Mutex::new(initial.master_addr),
        refresh: Duration::from_secs(config.redis_sentinel_refresh_secs.max(1)),
    };
    tokio::spawn(watcher.run());

    shared
}

struct SentinelState {
    client: Arc<Mutex<SentinelClient>>,
}

struct MasterConn {
    manager: ConnectionManager,
    master_addr: ConnectionAddr,
}

impl SentinelState {
    fn new(config: &AppConfig) -> redis::RedisResult<Self> {
        let nodes: Vec<ConnectionInfo> = config
            .redis_sentinel_hosts
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("redis://{s}").parse::<ConnectionInfo>())
            .collect::<Result<_, _>>()?;

        // Credentials applied to the *master* connection that Sentinel hands
        // back. Sentinel itself is unauthenticated in the gitops topology;
        // if that changes, the sentinel-side creds belong on the parsed
        // `redis://user:pass@host:26379` URLs above.
        let redis_settings = RedisConnectionInfo::default()
            .set_db(config.redis_database as i64)
            .set_username(&config.redis_username)
            .set_password(&config.redis_password);

        let node_info =
            SentinelNodeConnectionInfo::default().set_redis_connection_info(redis_settings);

        let client = SentinelClient::build(
            nodes,
            config.redis_sentinel_master.clone(),
            Some(node_info),
            SentinelServerType::Master,
        )?;

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
        })
    }

    async fn build_master_manager(&self) -> redis::RedisResult<MasterConn> {
        let master_client = self.client.lock().await.async_get_client().await?;
        let master_addr = master_client.get_connection_info().addr().clone();
        let manager = ConnectionManager::new(master_client).await?;
        Ok(MasterConn {
            manager,
            master_addr,
        })
    }
}

struct MasterWatcher {
    sentinel: SentinelState,
    shared: SharedConn,
    last_master: Mutex<ConnectionAddr>,
    refresh: Duration,
}

impl MasterWatcher {
    async fn run(self) {
        let mut tick = tokio::time::interval(self.refresh);
        // First tick fires immediately — skip it; we already built the manager.
        tick.tick().await;
        loop {
            tick.tick().await;
            match self.sentinel.build_master_manager().await {
                Ok(new) => {
                    let mut last = self.last_master.lock().await;
                    if *last == new.master_addr {
                        continue;
                    }
                    info!(
                        old_master = %*last,
                        new_master = %new.master_addr,
                        "Sentinel reports master changed; swapping ConnectionManager"
                    );
                    *self.shared.write().await = new.manager;
                    *last = new.master_addr;
                }
                Err(e) => {
                    warn!("Sentinel master lookup failed: {e}");
                }
            }
        }
    }
}
