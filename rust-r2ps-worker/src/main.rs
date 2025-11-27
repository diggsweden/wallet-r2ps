mod domain;
mod infrastructure;
mod application;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, instrument, warn};
use tracing_subscriber::{fmt, EnvFilter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

//use foyer::{Cache, CacheBuilder, EvictionConfig, LruConfig};
use crate::application::{ClientRepositorySpiPort, R2psService};
use crate::domain::{DefaultCipherSuite};
use crate::infrastructure::{KafkaConfig, R2psRequestKafkaMessageReceiver};
use crate::infrastructure::client_repository_memory_cache::ClientRepositoryMemoryCache;
use crate::infrastructure::r2ps_response_kafka_message_sender::R2psResponseKafkaMessageSender;
use crate::infrastructure::session_key_memory_cache::SessionKeyMemoryCache;

#[instrument(name="main", skip_all)]
fn main() {

    /*
    TODO

*/

    // init tracing
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_thread_ids(true)      // Include thread IDs
                .with_thread_names(true)    // Include thread names
                .with_target(false)         // Hide target (module path)
                .with_level(true)
                // Show log levels
        )
        .with(
            // Filter based on RUST_LOG env var, default to info
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    // config from env
    infrastructure::config::init();
    let cfg = KafkaConfig::init().unwrap();
    let help = KafkaConfig::get_help();
    info!("{:#?}", help);

    // Handle Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        r.store(false, Ordering::Relaxed);
    }).expect("Error setting Ctrl-C handler");

    // init server
    let r2ps_kafka_sender = R2psResponseKafkaMessageSender::new(&cfg.clone());
    let client_repository = Arc::new(ClientRepositoryMemoryCache::new());
    let session_key_cache = Arc::new(SessionKeyMemoryCache::new());
    let r2ps_service = R2psService::new(Arc::new(r2ps_kafka_sender), client_repository.clone(), session_key_cache.clone());
    let r2ps_kafka_receiver = R2psRequestKafkaMessageReceiver::new(&r2ps_service, running.clone());

    // start worker i.e. process requests to responses
    let join_handle = r2ps_kafka_receiver.start_worker_thread(cfg);

    // wait until worker thread finish
    join_handle.join().unwrap();

}

