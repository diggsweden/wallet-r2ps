use rust_r2ps_worker::run;
use tracing::instrument;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

#[instrument(name = "main", skip_all)]
fn main() {
    // init tracing
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_thread_ids(true) // Include thread IDs
                .with_thread_names(true) // Include thread names
                .with_target(false) // Hide target (module path)
                .with_level(true), // Show log levels
        )
        .with(
            // Filter based on RUST_LOG env var, default to info
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    run();
}
