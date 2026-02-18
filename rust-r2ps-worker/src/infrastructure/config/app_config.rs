use crate::application::OpaqueConfig;
use crate::infrastructure::{KafkaConfig, hsm_wrapper::Pkcs11Config};
use config::{Config, ConfigError, Environment};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server_private_key: String, // base64 env pem (double encoded)
    pub server_public_key: String,  // base64 env pem (double encoded)
    pub opaque_server_setup: Option<String>, // serialized OPAQUE server state, base64 encoded
    pub opaque_server_identifier: String, // stable domain identifier for OPAQUE protocol
    pub opaque_context: String,     // OPAQUE protocol context

    pub pkcs11_lib: String,
    pub pkcs11_slot_token_label: String,
    pub pkcs11_so_pin: Option<String>,
    pub pkcs11_user_pin: Option<String>,
    pub pkcs11_wrap_key_alias: String,
    /// bootstrap servers, comma separated list
    /// <https://github.com/confluentinc/librdkafka/blob/master/CONFIGURATION.md>
    pub kafka_bootstrap_servers: String,

    /// <https://github.com/confluentinc/librdkafka/blob/master/CONFIGURATION.md>
    pub kafka_broker_address_family: String,
    pub kafka_group_id: String,
    pub kafka_group_instance_id: String,
}

impl AppConfig {
    pub fn new() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        Config::builder()
            .set_default("kafka_group_id", "rust-grp")?
            .set_default("kafka_group_instance_id", "consumer-1")?
            .set_default("kafka_broker_address_family", "v4")?
            .set_default("opaque_server_identifier", "cloud-wallet.digg.se")?
            .set_default("opaque_context", "RPS-Ops")?
            .add_source(Environment::default())
            .build()?
            .try_deserialize()
    }
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
