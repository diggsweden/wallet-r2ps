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

    /// Number of concurrent worker tasks
    #[arg(short = 't', long, default_value = "4")]
    pub threads: usize,

    /// Mean time between requests per worker in milliseconds (0 = burst mode)
    #[arg(long, default_value = "100")]
    pub mean_delay_ms: u64,

    /// Test duration in seconds (0 = unlimited, Ctrl+C to stop)
    #[arg(short, long, default_value = "60")]
    pub duration_secs: u64,

    /// Number of HSM sign operations per authentication cycle
    #[arg(long, default_value = "5")]
    pub signs_per_cycle: usize,

    /// Stats reporting interval in seconds
    #[arg(long, default_value = "5")]
    pub stats_interval_secs: u64,
}
