// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "loadtest-kafka")]
#[command(about = "Direct-to-Kafka load tester for the R2PS wallet stack")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Onboard N clients by producing state-init / OPAQUE / generate-key
    /// requests directly to Kafka. Writes a test-data envelope identical to
    /// the one produced by `integration-load-tests generate`.
    Generate(GenerateArgs),
    /// Run a load test by replaying authenticate + sign cycles against the
    /// hsm-requests topic at high concurrency.
    LoadTest(LoadTestArgs),
}

#[derive(Parser, Clone)]
pub struct GenerateArgs {
    /// Kafka bootstrap servers (comma-separated)
    #[arg(long)]
    pub bootstrap_servers: String,

    /// Path to server EC public key PEM file
    #[arg(long)]
    pub server_pubkey_pem: String,

    /// Number of clients to generate
    #[arg(short = 'n', long, default_value = "10")]
    pub count: usize,

    /// PIN to use for all clients
    #[arg(long, default_value = "123456")]
    pub pin: String,

    /// Output file path (.json.gz)
    #[arg(short, long, default_value = "test-data.json.gz")]
    pub output: String,

    /// Number of concurrent client registrations
    #[arg(short, long, default_value = "16")]
    pub concurrency: usize,

    /// OPAQUE context string
    #[arg(long, default_value = "RPS-Ops")]
    pub opaque_context: String,

    /// OPAQUE server identifier
    #[arg(long, default_value = "dev.cloud-wallet.digg.se")]
    pub opaque_server_id: String,

    /// Device state TTL (ISO 8601 duration)
    #[arg(long, default_value = "P30D")]
    pub ttl: String,

    /// Kafka broker address family (v4 / v6)
    #[arg(long, default_value = "v4")]
    pub broker_address_family: String,

    /// Number of partitions for the per-process response topics
    #[arg(long, default_value = "8")]
    pub response_topic_partitions: i32,
}

#[derive(Parser, Clone)]
pub struct LoadTestArgs {
    /// Kafka bootstrap servers (comma-separated)
    #[arg(long)]
    pub bootstrap_servers: String,

    /// Path to server EC public key PEM file
    #[arg(long)]
    pub server_pubkey_pem: String,

    /// Path to test data file (.json.gz or .json)
    #[arg(long)]
    pub test_data: String,

    /// Limit to the first N clients from the dataset (0 = all)
    #[arg(long, default_value = "0")]
    pub clients: usize,

    /// Number of parallel auth+sign cycle loops per client. Each loop runs
    /// create_session followed by `signs_per_cycle` HSM sign ops, then
    /// restarts immediately. With `--inflight-per-client 4` and 60 clients
    /// the load test holds up to 240 concurrent cycle tasks in flight,
    /// allowing the Kafka producer to coalesce traffic into larger batches.
    #[arg(long, default_value = "1")]
    pub inflight_per_client: usize,

    /// Mean time between requests per loop in milliseconds (0 = no delay).
    #[arg(long, default_value = "0")]
    pub mean_delay_ms: u64,

    /// Sleep after a cycle error before retrying, in milliseconds. Set to
    /// 0 to retry immediately.
    #[arg(long, default_value = "0")]
    pub error_backoff_ms: u64,

    /// Test duration in seconds (0 = unlimited, Ctrl+C to stop)
    #[arg(short, long, default_value = "60")]
    pub duration_secs: u64,

    /// Number of HSM sign operations per authentication cycle
    #[arg(long, default_value = "1")]
    pub signs_per_cycle: usize,

    /// Stats reporting interval in seconds
    #[arg(long, default_value = "5")]
    pub stats_interval_secs: u64,

    /// Kafka broker address family (v4 / v6)
    #[arg(long, default_value = "v4")]
    pub broker_address_family: String,

    /// Per-request timeout in seconds
    #[arg(long, default_value = "30")]
    pub request_timeout_secs: u64,

    /// Number of partitions for the per-process response topics
    #[arg(long, default_value = "16")]
    pub response_topic_partitions: i32,

    /// Producer `linger.ms` — how long the producer waits to coalesce
    /// records into a batch. Higher = larger batches, higher per-record
    /// latency. Default 2 ms matches the in-cluster BFF.
    #[arg(long, default_value = "2")]
    pub producer_linger_ms: u32,

    /// Producer `batch.size` in bytes. Larger = more headroom before the
    /// producer flushes; defaults to 128 KiB.
    #[arg(long, default_value = "131072")]
    pub producer_batch_size_bytes: u32,

    /// Producer `compression.type` — one of `none`, `gzip`, `snappy`,
    /// `lz4`, `zstd`.
    #[arg(long, default_value = "lz4")]
    pub producer_compression: String,

    /// Producer `acks` — `0` (no broker ack, max throughput), `1`
    /// (leader-only, default), or `all`.
    #[arg(long, default_value = "1")]
    pub producer_acks: String,

    /// Pure producer benchmark: fire fresh OPAQUE `AuthenticateStart` (KE1)
    /// messages in a tight loop without awaiting the worker response. Useful
    /// for measuring the rate at which the client + Kafka can absorb traffic
    /// independent of worker latency. The worker's eventual responses arrive
    /// on the response topic but are discarded as orphans.
    #[arg(long, default_value_t = false)]
    pub produce_only: bool,
}
