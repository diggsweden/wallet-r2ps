// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use config::{Config, ConfigError, Environment};
use serde::Deserialize;

pub fn today_yyyymmdd() -> String {
    chrono::Utc::now().format("%Y%m%d").to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Kafka bootstrap servers (comma-separated)
    pub kafka_bootstrap_servers: String,
    /// Kafka consumer group ID
    pub kafka_group_id: String,
    /// Kafka broker address family (v4/v6)
    pub kafka_broker_address_family: String,

    /// Redis host
    pub redis_host: String,
    /// Redis port
    pub redis_port: u16,
    /// Redis username
    pub redis_username: String,
    /// Redis password
    pub redis_password: String,
    /// Redis database index
    pub redis_database: u8,

    /// HTTP server bind host
    pub server_host: String,
    /// HTTP server port
    pub server_port: u16,

    /// Enable synchronous response support (wait for Kafka reply inline)
    pub serve_sync: bool,
    /// Max milliseconds to wait for a synchronous response
    pub sync_timeout_ms: u64,
    /// Max milliseconds to wait for a state-init response
    pub state_init_timeout_ms: u64,
    /// Response cache TTL in seconds
    pub response_ttl_seconds: u64,
    /// Replay-protection: nonce TTL in seconds (default 600, matches session TTL)
    pub nonce_ttl_seconds: u64,
    /// URL template for the polling endpoint (%s = correlationId)
    pub response_events_template_url: String,
    /// Unique identifier for this BFF instance; used to name per-instance Kafka topics
    /// (`hsm-worker-responses-{id}-{YYYYMMDD}`, `state-init-responses-{id}-{YYYYMMDD}`).
    ///
    /// Override with BFF_INSTANCE_ID env var; defaults to a random UUID.
    ///
    /// In production (e.g. Kubernetes), set this to a stable, unique value such as the
    /// pod name via the downward API:
    ///   env:
    ///     - name: BFF_INSTANCE_ID
    ///       valueFrom:
    ///         fieldRef:
    ///           fieldPath: metadata.name
    ///
    /// On startup, each instance deletes empty per-instance topics older than today,
    /// bounding orphaned topic accumulation to one pair per calendar day.
    /// Each live instance also publishes a periodic heartbeat to its topics; once a
    /// dead instance's heartbeats have expired via `retention.ms`, the topics become
    /// empty and are cleaned up by the next startup.
    ///
    /// **Midnight race**: a pod started at 23:59:xx whose topics are deleted by a
    /// concurrent cleanup crossing into the new day will detect the failure on its
    /// first heartbeat, recreate the topics with the new date, and continue normally.
    pub bff_instance_id: String,
}

impl AppConfig {
    pub fn new() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        Config::builder()
            .set_default("kafka_group_id", "r2ps-rest-api-group")?
            .set_default("kafka_broker_address_family", "v4")?
            .set_default("redis_host", "localhost")?
            .set_default("redis_port", 6379)?
            .set_default("redis_username", "default")?
            .set_default("redis_password", "secret")?
            .set_default("redis_database", 0)?
            .set_default("server_host", "0.0.0.0")?
            .set_default("server_port", 8088)?
            .set_default("serve_sync", true)?
            .set_default("sync_timeout_ms", 3000)?
            .set_default("state_init_timeout_ms", 5000)?
            .set_default("response_ttl_seconds", 600)?
            .set_default("nonce_ttl_seconds", 600)?
            .set_default(
                "response_events_template_url",
                "http://localhost:8088/hsm/v1/requests/%s",
            )?
            .set_default("bff_instance_id", uuid::Uuid::new_v4().to_string())?
            .add_source(Environment::default())
            .build()?
            .try_deserialize()
    }

    pub fn hsm_worker_response_topic(&self) -> String {
        format!(
            "hsm-worker-responses-{}-{}",
            self.bff_instance_id,
            today_yyyymmdd()
        )
    }

    pub fn state_init_response_topic(&self) -> String {
        format!(
            "state-init-responses-{}-{}",
            self.bff_instance_id,
            today_yyyymmdd()
        )
    }

    pub fn redis_url(&self) -> String {
        format!(
            "redis://{}:{}@{}:{}/{}",
            self.redis_username,
            self.redis_password,
            self.redis_host,
            self.redis_port,
            self.redis_database,
        )
    }
}
