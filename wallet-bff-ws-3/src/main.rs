use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use wallet_bff_ws::{config::Config, initialize_app};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wallet_bff_ws=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load environment variables from .env file if present
    dotenvy::dotenv().ok();

    // Load configuration
    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Failed to load configuration: {}", e);
        std::process::exit(1);
    });

    tracing::info!("Starting R2PS REST API server");
    tracing::info!("Configuration loaded:");
    tracing::info!(
        "  Server: {}:{}{}",
        config.server.host,
        config.server.port,
        config.server.context_path
    );
    tracing::info!("  Redis: {}:{}", config.redis.host, config.redis.port);
    tracing::info!("  Kafka brokers: {}", config.kafka.brokers);
    tracing::info!(
        "  WebSocket: {}",
        if config.websocket.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );

    // Initialize application (composition root)
    let components = initialize_app(config.clone()).await?;

    // Start Kafka consumer in background
    let kafka_subscriber = components.kafka_subscriber;
    tokio::spawn(async move {
        kafka_subscriber.start().await;
    });

    // Start state-snapshot consumer in background
    let snapshot_consumer = components.snapshot_consumer;
    tokio::spawn(async move {
        snapshot_consumer.start().await;
    });
    tracing::info!("State-snapshot Kafka consumer started");

    // Start WebSocket Kafka consumer in background (if enabled)
    if let Some(ws_subscriber) = components.ws_kafka_subscriber {
        tokio::spawn(async move {
            ws_subscriber.start().await;
        });
        tracing::info!("WebSocket Kafka subscriber started");
    }

    // Create socket address
    let addr = SocketAddr::from((
        config
            .server
            .host
            .parse::<std::net::IpAddr>()
            .unwrap_or_else(|_| "0.0.0.0".parse().unwrap()),
        config.server.port,
    ));

    tracing::info!("Server listening on {}", addr);
    tracing::info!(
        "OpenAPI documentation available at http://{}/swagger-ui",
        addr
    );
    if config.websocket.enabled {
        tracing::info!(
            "WebSocket endpoint available at ws://{}{}/ws",
            addr,
            config.server.context_path
        );
    }

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, components.router).await?;

    Ok(())
}
