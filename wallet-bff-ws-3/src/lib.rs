// === Hexagonal Architecture Modules ===
// Domain: Pure business logic, no I/O, no framework dependencies
pub mod domain;
// Ports: Trait definitions (interfaces) for inbound and outbound dependencies
pub mod ports;
// Application: Use cases that orchestrate domain logic through ports
pub mod application;
// Adapters: Concrete implementations of ports (HTTP, Redis, Kafka)
pub mod adapters;

// === Cross-cutting Concerns ===
pub mod config;
pub mod error;
pub mod middleware;

use axum::{
    Router, middleware as axum_middleware,
    routing::{get, post},
};
use std::sync::Arc;
use std::time::Duration;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::adapters::{
    inbound::{
        http::{
            handlers::{api_info, health_check, new_state, poll_task, submit_request},
            state::HttpAppState,
        },
        ws::{
            auth::HpkeAuthContext,
            handler::ws_handler,
            registry::ClientConnectionRegistry,
            state::{SharedWsState, WsAppState},
        },
    },
    outbound::{
        kafka::{KafkaMessagePublisher, KafkaSubscriber, StateSnapshotConsumer, WsKafkaSubscriber},
        redis::{
            RedisClientKeyRepository, RedisConnection, RedisRequestRepository,
            RedisStateInitRepository,
        },
    },
};
use crate::application::{
    device_management::InitializeDeviceUseCase,
    hsm_integration::ProcessWorkerResponseUseCase,
    request_processing::{PollRequestUseCase, SubmitRequestUseCase},
};
use crate::config::Config;

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        adapters::inbound::http::handlers::submit_request,
        adapters::inbound::http::handlers::poll_task,
        adapters::inbound::http::handlers::new_state,
        adapters::inbound::http::handlers::api_info,
        adapters::inbound::http::handlers::health_check,
    ),
    components(
        schemas(
            adapters::inbound::http::dto::BffRequest,
            adapters::inbound::http::dto::AsyncResponseDto::<String>,
            adapters::inbound::http::dto::AsyncResponseStatus,
            adapters::inbound::http::dto::AsyncResponseError,
            adapters::inbound::http::dto::NewStateRequestDto,
            adapters::inbound::http::dto::NewStateResponseDto,
            adapters::inbound::http::dto::EcPublicJwk,
            adapters::inbound::http::dto::Link,
            adapters::inbound::http::dto::Links,
            adapters::inbound::http::dto::ApiInfoDto,
            error::ProblemDetail,
        )
    ),
    tags(
        (name = "R2PS API", description = "Remote to Physical Signing API endpoints"),
        (name = "Health", description = "Health check endpoints")
    ),
    info(
        title = "R2PS REST API",
        version = "0.1.0",
        description = "REST API Level 3 (HATEOAS) for Remote to Physical Signing service.\n\nREST API Niv\u{00e5} 3 (HATEOAS) f\u{00f6}r tj\u{00e4}nsten Remote to Physical Signing (R2PS). Tillhandah\u{00e5}lls av DIGG \u{2013} Myndigheten f\u{00f6}r digital f\u{00f6}rvaltning.\n\n## Known Issues / K\u{00e4}nda problem\n\n- Synchronous mode may time out for large payloads. Use asynchronous mode for requests expected to take longer than 5 seconds.\n- Synkront l\u{00e4}ge kan f\u{00e5} timeout vid stora nyttolaster. Anv\u{00e4}nd asynkront l\u{00e4}ge f\u{00f6}r f\u{00f6}rfr\u{00e5}gningar som f\u{00f6}rv\u{00e4}ntas ta l\u{00e4}ngre \u{00e4}n 5 sekunder.",
        contact(
            name = "DIGG \u{2013} Myndigheten f\u{00f6}r digital f\u{00f6}rvaltning",
            email = "info@digg.se",
            url = "https://www.digg.se"
        ),
        license(
            name = "EUPL-1.2",
            url = "https://joinup.ec.europa.eu/collection/eupl/eupl-text-eupl-12"
        )
    )
)]
struct ApiDoc;

/// Build the application router with all routes and middleware.
pub fn create_app(
    state: Arc<HttpAppState>,
    ws_state: Option<SharedWsState>,
    config: &Config,
) -> Router {
    // API routes following Swedish REST API Profile (RES.06)
    let mut api_router = Router::new()
        .route("/", post(submit_request))
        .route("/requests/:correlationId", get(poll_task))
        .route("/device-states", post(new_state))
        .route("/api-info", get(api_info))
        .route("/health", get(health_check))
        .with_state(state);

    // Add WebSocket route if enabled and configured
    if let Some(ws_state) = ws_state {
        let ws_router = Router::new()
            .route("/ws", get(ws_handler))
            .with_state(ws_state);
        api_router = api_router.merge(ws_router);
        tracing::info!("WebSocket endpoint enabled at /ws");
    }

    let openapi_url = format!("{}/openapi.json", config.server.context_path);
    let openapi_doc = enrich_openapi(ApiDoc::openapi());

    let mut app = Router::new()
        .nest(&config.server.context_path, api_router)
        .merge(SwaggerUi::new("/swagger-ui").url(openapi_url, openapi_doc))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    // Enforce HTTPS in production (SAK.01-SAK.03)
    if config.server.require_https {
        app = app.layer(axum_middleware::from_fn(middleware::require_https));
        tracing::info!("HTTPS enforcement enabled (required by Swedish REST API Profile)");
    } else {
        tracing::warn!("HTTPS enforcement disabled - only use for local development!");
    }

    // Configure CORS according to Swedish REST API Profile (SAK.17)
    if let Some(ref origins) = config.server.cors_allowed_origins {
        let origins: Vec<_> = origins
            .split(',')
            .map(|s| s.trim().parse().expect("Invalid CORS origin"))
            .collect();

        use tower_http::cors::AllowOrigin;
        let cors = tower_http::cors::CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
            ]);

        app = app.layer(cors);
        tracing::info!("CORS enabled for specific origins");
    } else {
        tracing::info!("CORS disabled (recommended for production)");
    }

    app
}

/// Type alias for the Kafka subscriber with concrete adapter types.
type ConcreteKafkaSubscriber = KafkaSubscriber<RedisRequestRepository, RedisStateInitRepository>;

/// Type alias for the WS Kafka subscriber.
type ConcreteWsKafkaSubscriber = WsKafkaSubscriber;

/// Type alias for the state snapshot consumer.
type ConcreteSnapshotConsumer = StateSnapshotConsumer<RedisClientKeyRepository>;

/// Result of application initialization.
pub struct AppComponents {
    pub router: Router,
    pub kafka_subscriber: Arc<ConcreteKafkaSubscriber>,
    pub ws_kafka_subscriber: Option<Arc<ConcreteWsKafkaSubscriber>>,
    pub snapshot_consumer: Arc<ConcreteSnapshotConsumer>,
}

/// Initialize all application components (composition root).
///
/// This is where dependency injection happens — concrete adapters are created
/// and wired into the use cases through their port traits.
pub async fn initialize_app(config: Config) -> anyhow::Result<AppComponents> {
    // -- Infrastructure: Redis Connection --
    let redis_url = build_redis_url(&config);
    let redis_conn = RedisConnection::new(&redis_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Redis: {}", e))?;

    // -- Outbound Adapters (implement port traits) --
    let key_repo = RedisClientKeyRepository::new(redis_conn.clone());
    let request_repo = RedisRequestRepository::new(
        redis_conn.clone(),
        Duration::from_secs(config.r2ps.response_ttl_seconds),
    );
    let state_init_repo = RedisStateInitRepository::new(redis_conn.clone());
    let publisher = KafkaMessagePublisher::new(&config.kafka)
        .map_err(|e| anyhow::anyhow!("Failed to create Kafka publisher: {}", e))?;

    // -- Application Use Cases (depend on port traits, injected with adapters) --
    let submit_request_use_case = SubmitRequestUseCase::new(
        key_repo.clone(),
        request_repo.clone(),
        publisher.clone(),
        config.r2ps.sync_timeout(),
    );

    let poll_request_use_case = PollRequestUseCase::new(request_repo.clone());

    let init_device_use_case = InitializeDeviceUseCase::new(
        key_repo.clone(),
        publisher.clone(),
        state_init_repo.clone(),
        Duration::from_secs(5),
    );

    // -- Inbound Adapter: HTTP State --
    let app_state = Arc::new(HttpAppState {
        submit_request_use_case: SubmitRequestUseCase::new(
            key_repo.clone(),
            request_repo.clone(),
            publisher.clone(),
            config.r2ps.sync_timeout(),
        ),
        poll_request_use_case,
        init_device_use_case,
        serve_sync: config.r2ps.serve_sync,
        base_url: config.server.base_url.clone(),
    });

    // -- Inbound Adapter: Kafka Consumer (r2ps-responses) --
    let worker_response_use_case = ProcessWorkerResponseUseCase::new(request_repo.clone());

    let kafka_subscriber = Arc::new(
        KafkaSubscriber::new(&config.kafka, worker_response_use_case, state_init_repo.clone())
            .map_err(|e| anyhow::anyhow!("Failed to create Kafka subscriber: {}", e))?,
    );

    // -- Inbound Adapter: State Snapshot Consumer --
    let snapshot_consumer = Arc::new(
        StateSnapshotConsumer::new(&config.kafka, key_repo.clone())
            .map_err(|e| anyhow::anyhow!("Failed to create snapshot consumer: {}", e))?,
    );

    // -- WebSocket Components (optional) --
    let (ws_state, ws_kafka_subscriber) = if config.websocket.enabled {
        let ws_config = &config.websocket;

        let server_priv_jwk = ws_config
            .server_private_key_jwk()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "WebSocket enabled but WEBSOCKET__SERVER_PRIVATE_KEY_B64 not configured"
                )
            })?
            .map_err(|e| anyhow::anyhow!("Failed to decode server private key: {}", e))?;

        let server_pub_jwk = ws_config
            .server_public_key_jwk()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "WebSocket enabled but WEBSOCKET__SERVER_PUBLIC_KEY_B64 not configured"
                )
            })?
            .map_err(|e| anyhow::anyhow!("Failed to decode server public key: {}", e))?;

        let hpke_auth = HpkeAuthContext::from_jwk(
            &server_priv_jwk,
            &server_pub_jwk,
            ws_config.server_kid.clone(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create HPKE auth context: {}", e))?;

        let registry = ClientConnectionRegistry::new();

        // Create WS-specific Kafka subscriber with unique group_id per pod
        let ws_subscriber = Arc::new(
            WsKafkaSubscriber::new(
                &config.kafka,
                &ws_config.kafka_group_id_prefix,
                registry.clone(),
            )
            .map_err(|e| anyhow::anyhow!("Failed to create WS Kafka subscriber: {}", e))?,
        );

        let ws_app_state: SharedWsState = Arc::new(WsAppState {
            hpke_auth,
            registry,
            submit_request_use_case,
            key_repo: key_repo.clone(),
        });

        tracing::info!("WebSocket support initialized");
        (Some(ws_app_state), Some(ws_subscriber))
    } else {
        tracing::info!("WebSocket support disabled");
        (None, None)
    };

    // -- Build Router --
    let app = create_app(app_state, ws_state, &config);

    Ok(AppComponents {
        router: app,
        kafka_subscriber,
        ws_kafka_subscriber,
        snapshot_consumer,
    })
}

/// Enrich the generated OpenAPI document with fields required by Swedish REST API Profile.
///
/// Adds `terms_of_service` (DOK.03), `x-service-level` (DOK.08), and `x-known-issues` (DOK.09).
/// These cannot be set via the `#[openapi]` macro in utoipa v4, so we mutate post-generation.
fn enrich_openapi(mut doc: utoipa::openapi::OpenApi) -> utoipa::openapi::OpenApi {
    // DOK.03: termsOfService (not supported in utoipa v4 macro)
    doc.info.terms_of_service =
        Some("https://www.digg.se/digitala-tjanster/e-legitimering/tillitsramverk".to_string());

    // DOK.08 & DOK.09: Service level and known issues as info-level extensions
    let extensions = doc
        .info
        .extensions
        .get_or_insert_with(std::collections::HashMap::new);
    extensions.insert(
        "x-service-level".to_string(),
        serde_json::json!({
            "description": "This API is currently in alpha. No SLA guarantees are provided. The service targets 99% availability during business hours (CET/CEST).",
            "beskrivning": "Detta API \u{00e4}r f\u{00f6}r n\u{00e4}rvarande i alfa. Inga SLA-garantier ges. Tj\u{00e4}nsten siktar p\u{00e5} 99% tillg\u{00e4}nglighet under kontorstid (CET/CEST)."
        }),
    );
    extensions.insert(
        "x-known-issues".to_string(),
        serde_json::json!([
            {
                "description": "Synchronous mode may time out for large payloads. Use asynchronous mode for requests expected to take longer than 5 seconds.",
                "beskrivning": "Synkront l\u{00e4}ge kan f\u{00e5} timeout vid stora nyttolaster. Anv\u{00e4}nd asynkront l\u{00e4}ge f\u{00f6}r f\u{00f6}rfr\u{00e5}gningar som f\u{00f6}rv\u{00e4}ntas ta l\u{00e4}ngre \u{00e4}n 5 sekunder."
            }
        ]),
    );
    doc
}

fn build_redis_url(config: &Config) -> String {
    match config.redis.password.as_deref() {
        Some(pw) if !pw.is_empty() => {
            format!(
                "redis://:{}@{}:{}/{}",
                pw, config.redis.host, config.redis.port, config.redis.db
            )
        }
        _ => {
            format!(
                "redis://{}:{}/{}",
                config.redis.host, config.redis.port, config.redis.db
            )
        }
    }
}
