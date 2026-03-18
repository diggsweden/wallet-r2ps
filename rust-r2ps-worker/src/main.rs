use clap::{Parser, Subcommand};
use rust_r2ps_worker::run;
use tracing::instrument;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser)]
#[command(name = "hsm-worker", version, about = "R2PS HSM Worker")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the worker (default)
    Run,
    /// Bootstrap the state-snapshot topic from PostgreSQL
    BootstrapSnapshot {
        /// Optional: filter to a specific device_id
        #[arg(long)]
        device_id: Option<String>,
    },
}

#[instrument(name = "main", skip_all)]
fn main() {
    dotenvy::dotenv().ok();

    // init tracing
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(false)
                .with_level(true),
        )
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => {
            run();
        }
        Commands::BootstrapSnapshot { device_id } => {
            rust_r2ps_worker::bootstrap_snapshot(device_id);
        }
    }
}
