// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "integration-load-tests")]
#[command(about = "Integration and load testing tool for the R2PS wallet system")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Generate test data by registering clients through the BFF
    Generate(GenerateArgs),
    /// Run load tests against the BFF using pre-generated test data
    LoadTest(LoadTestArgs),
}

#[derive(Parser, Clone)]
pub struct GenerateArgs {
    /// BFF base URL (e.g. http://localhost:8088)
    #[arg(long)]
    pub bff_url: String,

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
    #[arg(short, long, default_value = "4")]
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
}

#[derive(Parser, Clone)]
pub struct LoadTestArgs {
    /// BFF base URL (e.g. http://localhost:8088)
    #[arg(long)]
    pub bff_url: String,

    /// Path to server EC public key PEM file
    #[arg(long)]
    pub server_pubkey_pem: String,

    /// Path to test data file (.json.gz or .json)
    #[arg(long)]
    pub test_data: String,

    /// Number of concurrent worker tasks (closed-loop mode).
    /// Each task runs cycles sequentially: at most `threads` cycles in flight.
    /// Ignored when --target-rps > 0 (open-loop mode is used instead).
    #[arg(short = 't', long, default_value = "4")]
    pub threads: usize,

    /// Mean time between requests per worker in milliseconds (0 = burst mode).
    /// Closed-loop mode only.
    #[arg(long, default_value = "100")]
    pub mean_delay_ms: u64,

    /// Target auth cycles per second (open-loop mode).
    /// When > 0, replaces closed-loop --threads pacing with a single
    /// rate-limited producer that fire-and-forget spawns each cycle as
    /// an independent tokio task. A single loadtest pod with a couple
    /// of CPU cores can sustain thousands of in-flight cycles this way.
    #[arg(long, default_value = "0")]
    pub target_rps: u64,

    /// Maximum in-flight cycles in open-loop mode. The producer reports
    /// saturation (and drops the tick) when this many cycles are
    /// already in progress, which surfaces the system-under-test's
    /// throughput ceiling cleanly. Ignored in closed-loop mode.
    #[arg(long, default_value = "10000")]
    pub max_concurrent: usize,

    /// Test duration in seconds (0 = unlimited, Ctrl+C to stop)
    #[arg(short, long, default_value = "60")]
    pub duration_secs: u64,

    /// Number of HSM sign operations per authentication cycle
    #[arg(long, default_value = "1")]
    pub signs_per_cycle: usize,

    /// Stats reporting interval in seconds
    #[arg(long, default_value = "5")]
    pub stats_interval_secs: u64,
}
