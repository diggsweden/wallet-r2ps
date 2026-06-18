// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use config::{Config, ConfigError, Environment};
use hsm_common::Curve;
use serde::Deserialize;

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
    /// Comma-separated `host:port` sentinel nodes. When set together with
    /// [`redis_sentinel_master`], the BFF resolves the current master via
    /// Sentinel instead of connecting directly to [`redis_host`].
    pub redis_sentinel_hosts: String,
    /// Sentinel-managed master (service) name. Required when
    /// [`redis_sentinel_hosts`] is set.
    pub redis_sentinel_master: String,

    /// HTTP server bind host
    pub server_host: String,
    /// HTTP server port
    pub server_port: u16,

    /// Max seconds a GET handler will block (BLPOP) waiting for the response.
    /// Clients must keep re-polling after a 202.
    pub long_poll_timeout_seconds: u64,
    /// TTL for cached response envelopes in the Redis response store.
    pub response_ttl_seconds: u64,
    /// Replay-protection: nonce TTL in seconds (default 600, matches session TTL)
    pub nonce_ttl_seconds: u64,
    /// URL template for the request polling endpoint (%s = correlationId)
    pub response_events_template_url: String,
    /// URL template for the state-init polling endpoint (%s = correlationId)
    pub state_init_events_template_url: String,

    /// Kafka topic for HSM worker responses directed to this instance.
    pub hsm_worker_response_topic: String,

    /// Kafka topic for state-init responses directed to this instance.
    pub state_init_response_topic: String,
    /// Default curve for initial HSM key generation when the client does not specify one (e.g. "P-256")
    pub default_initial_key_curve: Curve,
}

impl AppConfig {
    pub fn new() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        let pod_name = std::env::var("POD_NAME").unwrap_or_else(|_| "default".to_string());
        Config::builder()
            .set_default("kafka_group_id", "r2ps-rest-api-group")?
            .set_default("kafka_broker_address_family", "v4")?
            .set_default("redis_host", "localhost")?
            .set_default("redis_port", 6379)?
            .set_default("redis_username", "default")?
            .set_default("redis_password", "secret")?
            .set_default("redis_database", 0)?
            .set_default("redis_sentinel_hosts", "")?
            .set_default("redis_sentinel_master", "")?
            .set_default("server_host", "0.0.0.0")?
            .set_default("server_port", 8088)?
            .set_default("long_poll_timeout_seconds", 25)?
            .set_default("response_ttl_seconds", 600)?
            .set_default("nonce_ttl_seconds", 600)?
            .set_default(
                "response_events_template_url",
                "http://localhost:8088/hsm/v1/requests/%s",
            )?
            .set_default(
                "state_init_events_template_url",
                "http://localhost:8088/hsm/v1/device-states/%s",
            )?
            .set_default("default_initial_key_curve", "P-256")?
            .set_default(
                "hsm_worker_response_topic",
                format!("hsm-worker-responses-{pod_name}"),
            )?
            .set_default(
                "state_init_response_topic",
                format!("state-init-responses-{pod_name}"),
            )?
            .add_source(Environment::default())
            .build()?
            .try_deserialize()
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
