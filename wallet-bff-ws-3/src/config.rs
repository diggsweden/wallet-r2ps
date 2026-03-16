use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub redis: RedisConfig,
    pub kafka: KafkaConfig,
    pub r2ps: R2psConfig,
    #[serde(default)]
    pub websocket: WebSocketConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_context_path")]
    pub context_path: String,

    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Comma-separated list of allowed CORS origins (SAK.17)
    /// Leave empty to disable CORS. Use specific origins, never "*"
    #[serde(default)]
    pub cors_allowed_origins: Option<String>,

    /// Require HTTPS in production (SAK.01-SAK.03)
    #[serde(default = "default_require_https")]
    pub require_https: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisConfig {
    #[serde(default = "default_redis_host")]
    pub host: String,

    #[serde(default = "default_redis_port")]
    pub port: u16,

    #[serde(default)]
    pub password: Option<String>,

    #[serde(default = "default_redis_db")]
    pub db: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KafkaConfig {
    #[serde(default = "default_kafka_brokers")]
    pub brokers: String,

    /// `broker.address.family` for librdkafka – set to `"v4"` to force IPv4,
    /// avoiding silent consumer failures in Docker networks that advertise
    /// IPv6 addresses.  Matches the setting used by `rust-r2ps-worker`.
    #[serde(default = "default_broker_address_family")]
    pub broker_address_family: String,

    pub consumer: KafkaConsumerConfig,
    pub producer: KafkaProducerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KafkaConsumerConfig {
    #[serde(default = "default_group_id")]
    pub group_id: String,

    #[serde(default = "default_r2ps_response_topic")]
    pub r2ps_response_topic: String,

    /// Log-compacted topic with full `DeviceHsmState` snapshots.
    /// Consumed to populate the Valkey client key cache.
    #[serde(default = "default_state_snapshot_topic")]
    pub state_snapshot_topic: String,

    /// `fetch.wait.max.ms` — maximum time the broker waits for
    /// `fetch.min.bytes` before returning a fetch response.
    /// Lower values reduce consumer latency at the cost of more
    /// frequent fetch requests. Default: 50ms (librdkafka default is 500ms).
    #[serde(default = "default_fetch_wait_max_ms")]
    pub fetch_wait_max_ms: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KafkaProducerConfig {
    #[serde(default = "default_acks")]
    pub acks: String,

    #[serde(default = "default_retries")]
    pub retries: u32,

    /// `linger.ms` — how long to wait for more messages before sending
    /// a batch. 0 = send immediately (no batching delay). Under load,
    /// natural batching still occurs. Default: 0ms (librdkafka default is 5ms).
    #[serde(default = "default_linger_ms")]
    pub linger_ms: u32,

    /// `socket.nagle.disable` — disable Nagle's algorithm to avoid
    /// TCP-level coalescing delay. Default: true.
    #[serde(default = "default_socket_nagle_disable")]
    pub socket_nagle_disable: bool,

    /// All commands (both regular and state-init) go to this topic.
    #[serde(default = "default_r2ps_request_topic")]
    pub r2ps_request_topic: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct R2psConfig {
    #[serde(default = "default_serve_sync")]
    pub serve_sync: bool,

    #[serde(default = "default_sync_timeout_ms")]
    pub sync_timeout_ms: u64,

    #[serde(default = "default_response_ttl_seconds")]
    pub response_ttl_seconds: u64,
}

impl R2psConfig {
    pub fn sync_timeout(&self) -> Duration {
        Duration::from_millis(self.sync_timeout_ms)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebSocketConfig {
    /// Enable WebSocket endpoint
    #[serde(default = "default_ws_enabled")]
    pub enabled: bool,

    /// Server EC P-256 private key as base64url-encoded JWK JSON (for HPKE mutual auth).
    /// Encode with: echo -n '{"kty":"EC",...}' | base64 -w0 | tr '+/' '-_' | tr -d '='
    #[serde(default)]
    pub server_private_key_b64: Option<String>,

    /// Server EC P-256 public key as base64url-encoded JWK JSON (advertised to clients).
    #[serde(default)]
    pub server_public_key_b64: Option<String>,

    /// Server key identifier (kid)
    #[serde(default = "default_server_kid")]
    pub server_kid: String,

    /// Kafka consumer group_id prefix for WebSocket fan-out.
    /// A UUID is appended per pod instance to ensure all pods receive all messages.
    #[serde(default = "default_ws_group_id_prefix")]
    pub kafka_group_id_prefix: String,

    /// Authentication handshake timeout in milliseconds
    #[serde(default = "default_auth_timeout_ms")]
    pub auth_timeout_ms: u64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: default_ws_enabled(),
            server_private_key_b64: None,
            server_public_key_b64: None,
            server_kid: default_server_kid(),
            kafka_group_id_prefix: default_ws_group_id_prefix(),
            auth_timeout_ms: default_auth_timeout_ms(),
        }
    }
}

impl WebSocketConfig {
    pub fn auth_timeout(&self) -> Duration {
        Duration::from_millis(self.auth_timeout_ms)
    }

    /// Decode the base64url-encoded server private key JWK.
    pub fn server_private_key_jwk(&self) -> Option<Result<String, String>> {
        self.server_private_key_b64
            .as_ref()
            .map(|b64| decode_b64_jwk(b64))
    }

    /// Decode the base64url-encoded server public key JWK.
    pub fn server_public_key_jwk(&self) -> Option<Result<String, String>> {
        self.server_public_key_b64
            .as_ref()
            .map(|b64| decode_b64_jwk(b64))
    }
}

/// Decode a base64url-encoded (no padding) JWK JSON string.
fn decode_b64_jwk(b64: &str) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64.trim())
        .or_else(|_| {
            // Also try standard base64 with padding for flexibility
            base64::engine::general_purpose::STANDARD.decode(b64.trim())
        })
        .map_err(|e| format!("invalid base64: {}", e))?;
    String::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {}", e))
}

// Default values
fn default_port() -> u16 {
    8088
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_context_path() -> String {
    "/r2ps-api/v1".to_string()
}

fn default_base_url() -> String {
    "http://localhost:8088/r2ps-api/v1".to_string()
}

fn default_redis_host() -> String {
    "localhost".to_string()
}

fn default_redis_port() -> u16 {
    6379
}

fn default_redis_db() -> i64 {
    0
}

fn default_kafka_brokers() -> String {
    "localhost:9092".to_string()
}

fn default_broker_address_family() -> String {
    "v4".to_string()
}

fn default_group_id() -> String {
    "wallet-bff-ws-group".to_string()
}

fn default_r2ps_response_topic() -> String {
    "r2ps-responses".to_string()
}

fn default_state_snapshot_topic() -> String {
    "state-snapshot".to_string()
}

fn default_fetch_wait_max_ms() -> u32 {
    50
}

fn default_acks() -> String {
    "all".to_string()
}

fn default_retries() -> u32 {
    3
}

fn default_linger_ms() -> u32 {
    0
}

fn default_socket_nagle_disable() -> bool {
    true
}

fn default_r2ps_request_topic() -> String {
    "r2ps-requests".to_string()
}

fn default_serve_sync() -> bool {
    true
}

fn default_sync_timeout_ms() -> u64 {
    3000
}

fn default_response_ttl_seconds() -> u64 {
    600 // 10 minutes
}

fn default_require_https() -> bool {
    false // Default false for local development. Set to true in production!
}

fn default_ws_enabled() -> bool {
    false
}

fn default_server_kid() -> String {
    "r2ps-server-key-1".to_string()
}

fn default_ws_group_id_prefix() -> String {
    "r2ps-ws".to_string()
}

fn default_auth_timeout_ms() -> u64 {
    5000
}

impl Config {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        let settings = config::Config::builder()
            .add_source(config::Environment::default().separator("__"))
            .build()?;

        settings.try_deserialize()
    }
}
