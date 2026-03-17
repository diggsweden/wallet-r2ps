use crate::application::OpaqueConfig;
use crate::infrastructure::{hsm_wrapper::Pkcs11Config, KafkaConfig};
use config::{Config, ConfigError, Environment};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server_private_key: String,
    pub server_public_key: String,
    pub opaque_server_setup: Option<String>,
    pub opaque_server_identifier: String,
    pub opaque_context: String,

    pub pkcs11_lib: String,
    pub pkcs11_slot_token_label: String,
    pub pkcs11_so_pin: Option<String>,
    pub pkcs11_user_pin: Option<String>,
    pub pkcs11_wrap_key_alias: String,

    pub kafka_bootstrap_servers: String,
    pub kafka_broker_address_family: String,
    pub kafka_group_id: String,
    pub kafka_group_instance_id: String,

    // PostgreSQL configuration
    pub postgres_host: String,
    pub postgres_port: u16,
    pub postgres_db: String,
    pub postgres_user: String,
    pub postgres_password: String,

    // Outbox relay configuration
    pub outbox_poll_timeout_ms: u64,

    // State cache configuration
    pub state_cache_path: String,
    pub pod_id: String,
    pub state_cache_capacity: u64,
    pub state_cache_ttl_secs: u64,
    pub catchup_workers: usize,
}

/// PostgreSQL connection config helper.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub db: String,
    pub user: String,
    pub password: String,
}

impl PostgresConfig {
    pub fn connection_string(&self) -> String {
        format!(
            "host={} port={} dbname={} user={} password={}",
            self.host, self.port, self.db, self.user, self.password
        )
    }
}

impl From<&AppConfig> for PostgresConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            host: config.postgres_host.clone(),
            port: config.postgres_port,
            db: config.postgres_db.clone(),
            user: config.postgres_user.clone(),
            password: config.postgres_password.clone(),
        }
    }
}

impl AppConfig {
    pub fn new() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        let default_catchup_workers = std::cmp::max(1, num_cpus::get().saturating_sub(2));

        Config::builder()
            .set_default("kafka_group_id", "rust-grp")?
            .set_default("kafka_group_instance_id", "consumer-1")?
            .set_default("kafka_broker_address_family", "v4")?
            .set_default("opaque_server_identifier", "cloud-wallet.digg.se")?
            .set_default("opaque_context", "RPS-Ops")?
            .set_default("postgres_host", "localhost")?
            .set_default("postgres_port", 5432)?
            .set_default("postgres_db", "r2ps")?
            .set_default("postgres_user", "r2ps")?
            .set_default("postgres_password", "secret")?
            .set_default("outbox_poll_timeout_ms", 5000i64)?
            .set_default("state_cache_path", "/tmp/r2ps-tamper-cache.redb")?
            .set_default("pod_id", gethostname())?
            .set_default("state_cache_capacity", 1_000_000i64)?
            .set_default("state_cache_ttl_secs", 3600i64)?
            .set_default("catchup_workers", default_catchup_workers as i64)?
            .add_source(Environment::default())
            .build()?
            .try_deserialize()
    }

    pub fn postgres_config(&self) -> PostgresConfig {
        PostgresConfig::from(self)
    }
}

fn gethostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
}

impl From<AppConfig> for KafkaConfig {
    fn from(value: AppConfig) -> Self {
        Self {
            bootstrap_servers: value.kafka_bootstrap_servers,
            broker_address_family: value.kafka_broker_address_family,
            group_id: value.kafka_group_id,
            group_instance_id: value.kafka_group_instance_id,
        }
    }
}

impl From<AppConfig> for Pkcs11Config {
    fn from(val: AppConfig) -> Self {
        Self {
            lib_path: val.pkcs11_lib,
            slot_token_label: val.pkcs11_slot_token_label,
            so_pin: val.pkcs11_so_pin,
            user_pin: val.pkcs11_user_pin,
            wrap_key_alias: val.pkcs11_wrap_key_alias,
        }
    }
}

impl From<AppConfig> for OpaqueConfig {
    fn from(value: AppConfig) -> Self {
        Self {
            opaque_server_setup: value.opaque_server_setup,
            opaque_context: value.opaque_context,
            opaque_server_identifier: value.opaque_server_identifier,
        }
    }
}
