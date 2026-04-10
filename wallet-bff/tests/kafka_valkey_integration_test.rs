// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::Arc;
use std::time::Duration;

use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::{ClientConfig, Message};
use redis::aio::ConnectionManager;
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::kafka::apache::{self, Kafka};
use testcontainers_modules::valkey::Valkey;
use tokio::sync::Mutex;

use wallet_bff::application::port::incoming::ResponseUseCase;
use wallet_bff::application::port::outgoing::{
    DeviceStatePort, PendingContextPort, ResponseSinkPort,
};
use wallet_bff::application::service::ResponseService;
use wallet_bff::domain::{
    CachedResponse, EcPublicJwk, HsmWorkerRequest, HsmWorkerResponse, OuterRequest, OuterResponse,
    PendingRequestContext, StateInitRequest, StateInitResponse, Status, TypedJws,
};
use wallet_bff::infrastructure::adapters::incoming::kafka::state_init_cache::StateInitResponseCache;
use wallet_bff::infrastructure::adapters::incoming::kafka::{
    r2ps_response_consumer, state_init_response_consumer,
};
use wallet_bff::infrastructure::adapters::outgoing::kafka::request_sender::{
    KafkaRequestSender, KafkaStateInitSender,
};
use wallet_bff::infrastructure::adapters::outgoing::redis::device_state::DeviceStateRedisAdapter;
use wallet_bff::infrastructure::adapters::outgoing::redis::pending_context::PendingContextRedisAdapter;
use wallet_bff::infrastructure::adapters::outgoing::redis::response_sink::ResponseSinkRedisAdapter;

// ── Container helpers ────────────────────────────────────────────────────────

async fn start_kafka() -> (ContainerAsync<Kafka>, String) {
    let container = Kafka::default().start().await.unwrap();
    let port = container
        .get_host_port_ipv4(apache::KAFKA_PORT)
        .await
        .unwrap();
    let bootstrap = format!("127.0.0.1:{}", port);
    (container, bootstrap)
}

async fn start_valkey() -> (ContainerAsync<Valkey>, String) {
    let container = Valkey::default().start().await.unwrap();
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://localhost:{}", port);
    (container, url)
}

async fn valkey_connection_manager(url: &str) -> ConnectionManager {
    let client = redis::Client::open(url).expect("Failed to create Valkey client");
    ConnectionManager::new(client)
        .await
        .expect("Failed to create ConnectionManager")
}

// ── Valkey adapter tests ─────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_device_state_valkey_round_trip() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = DeviceStateRedisAdapter::new(conn);

    adapter.save("dev-1", "state-jws-value", 60).await;
    let loaded = adapter.load("dev-1").await;
    assert_eq!(loaded, Some("state-jws-value".to_string()));

    let missing = adapter.load("nonexistent").await;
    assert_eq!(missing, None);
}

#[tokio::test]
#[ignore]
async fn test_pending_context_valkey_round_trip() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = PendingContextRedisAdapter::new(conn);

    let ctx = PendingRequestContext {
        state_key: "device-abc".to_string(),
        ttl_seconds: 300,
    };

    adapter.save("req-1", &ctx).await;
    let loaded = adapter.load("req-1").await;
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.state_key, "device-abc");
    assert_eq!(loaded.ttl_seconds, 300);

    let missing = adapter.load("nonexistent").await;
    assert!(missing.is_none());
}

#[tokio::test]
#[ignore]
async fn test_response_sink_valkey_round_trip() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = ResponseSinkRedisAdapter::new(conn, 60);

    let response = CachedResponse {
        request_id: "req-42".to_string(),
        state_jws: Some("updated-state".to_string()),
        outer_response_jws: Some(TypedJws::<OuterResponse>::new("outer-resp".to_string())),
        status: Status::Ok,
        error_message: None,
    };

    adapter.store(&response).await;
    let loaded = adapter.load("req-42").await;
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.request_id, "req-42");
    assert_eq!(loaded.status, Status::Ok);
    assert_eq!(loaded.state_jws, Some("updated-state".to_string()));

    let missing = adapter.load("nonexistent").await;
    assert!(missing.is_none());
}

// Verifies that save() passes the TTL through to Valkey: a key saved with a 1 s
// TTL must be gone after 2 s.
#[tokio::test]
#[ignore]
async fn test_device_state_valkey_ttl_expiry() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = DeviceStateRedisAdapter::new(conn);

    adapter.save("ttl-key", "value", 1).await;
    assert_eq!(adapter.load("ttl-key").await, Some("value".to_string()));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(adapter.load("ttl-key").await, None);
}

// Verifies that store() passes response_ttl_seconds through to Valkey: a response
// stored with a 1 s TTL must be gone after 2 s.
#[tokio::test]
#[ignore]
async fn test_response_sink_valkey_ttl_expiry() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = ResponseSinkRedisAdapter::new(conn, 1);

    let response = CachedResponse {
        request_id: "ttl-req".to_string(),
        state_jws: None,
        outer_response_jws: None,
        status: Status::Ok,
        error_message: None,
    };
    adapter.store(&response).await;
    assert!(adapter.load("ttl-req").await.is_some());
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(adapter.load("ttl-req").await.is_none());
}

// Verifies that a second save() on the same key replaces the previous value.
// The device state machine writes to the same key at each step (Registered →
// Authenticated), so silent no-op or append behaviour would be a data bug.
#[tokio::test]
#[ignore]
async fn test_device_state_valkey_overwrite() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = DeviceStateRedisAdapter::new(conn);

    adapter.save("dev-1", "first-state", 60).await;
    adapter.save("dev-1", "second-state", 60).await;
    assert_eq!(
        adapter.load("dev-1").await,
        Some("second-state".to_string())
    );
}

// ── Kafka producer tests ─────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_request_sender_produces_to_kafka() {
    let (_container, bootstrap) = start_kafka().await;

    let sender = KafkaRequestSender::new(&bootstrap, "v4");
    let request = HsmWorkerRequest {
        request_id: "req-100".to_string(),
        state_jws: "test-state-jws".to_string(),
        outer_request_jws: TypedJws::<OuterRequest>::new("test-outer-jws".to_string()),
    };

    use wallet_bff::application::port::outgoing::RequestSenderPort;
    sender
        .send(&request, "device-1")
        .await
        .expect("send failed");

    // Consume and verify
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("group.id", "test-request-sender")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("consumer creation failed");

    consumer.subscribe(&["r2ps-requests"]).unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(30), consumer.recv())
        .await
        .expect("timeout waiting for message")
        .expect("consumer error");

    let payload = msg.payload().expect("empty payload");
    let received: HsmWorkerRequest = serde_json::from_slice(payload).expect("deserialize failed");
    assert_eq!(received.request_id, "req-100");
    assert_eq!(received.state_jws, "test-state-jws");
}

#[tokio::test]
#[ignore]
async fn test_state_init_sender_produces_to_kafka() {
    let (_container, bootstrap) = start_kafka().await;

    let sender = KafkaStateInitSender::new(&bootstrap, "v4");
    let request = StateInitRequest {
        request_id: "req-200".to_string(),
        public_key: test_ec_jwk(),
    };

    use wallet_bff::application::port::outgoing::StateInitSenderPort;
    sender
        .send(&request, "device-2")
        .await
        .expect("send failed");

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("group.id", "test-state-init-sender")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("consumer creation failed");

    consumer.subscribe(&["state-init-requests"]).unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(30), consumer.recv())
        .await
        .expect("timeout")
        .expect("consumer error");

    let payload = msg.payload().expect("empty payload");
    let received: StateInitRequest = serde_json::from_slice(payload).expect("deserialize failed");
    assert_eq!(received.request_id, "req-200");
    assert_eq!(received.public_key.crv, "P-256");
}

// ── Kafka consumer tests ─────────────────────────────────────────────────────

/// Test-local capturing implementation of ResponseUseCase.
struct CapturingResponseUseCase {
    responses: Mutex<Vec<HsmWorkerResponse>>,
}

impl CapturingResponseUseCase {
    fn new() -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl ResponseUseCase for CapturingResponseUseCase {
    async fn response_ready(&self, response: HsmWorkerResponse) {
        self.responses.lock().await.push(response);
    }

    async fn wait_for_response(&self, _: &str, _: u64) -> Option<CachedResponse> {
        None
    }
}

#[tokio::test]
#[ignore]
async fn test_response_consumer_receives_and_calls_use_case() {
    let (_container, bootstrap) = start_kafka().await;

    let capturing = Arc::new(CapturingResponseUseCase::new());
    let group_id = format!("it-{}", uuid::Uuid::new_v4());

    // Produce before starting the consumer — auto.offset.reset=earliest means the
    // consumer will replay this message on its first poll without needing a sleep.
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("producer creation failed");

    let response = HsmWorkerResponse {
        request_id: "req-300".to_string(),
        state_jws: Some("state".to_string()),
        outer_response_jws: Some(TypedJws::<OuterResponse>::new("outer".to_string())),
        status: Status::Ok,
        error_message: None,
    };
    let payload = serde_json::to_string(&response).unwrap();

    producer
        .send(
            FutureRecord::to("r2ps-responses")
                .key("device-1")
                .payload(&payload),
            Duration::from_secs(5),
        )
        .await
        .expect("produce failed");

    r2ps_response_consumer::start(
        &bootstrap,
        &group_id,
        capturing.clone() as Arc<dyn ResponseUseCase>,
    );

    // Wait for the consumer to process the message
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let captured = capturing.responses.lock().await;
        if !captured.is_empty() {
            assert_eq!(captured[0].request_id, "req-300");
            assert_eq!(captured[0].status, Status::Ok);
            break;
        }
        drop(captured);
        if tokio::time::Instant::now() >= deadline {
            panic!("Timeout waiting for consumer to receive message");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore]
async fn test_state_init_consumer_receives_and_saves_to_valkey() {
    let (_kafka, bootstrap) = start_kafka().await;
    let (_valkey, valkey_url) = start_valkey().await;
    let conn = valkey_connection_manager(&valkey_url).await;

    let device_state_port: Arc<dyn DeviceStatePort> =
        Arc::new(DeviceStateRedisAdapter::new(conn.clone()));
    let pending_context_port: Arc<dyn PendingContextPort> =
        Arc::new(PendingContextRedisAdapter::new(conn.clone()));
    let cache = Arc::new(StateInitResponseCache::new());

    // Pre-save a pending context
    let request_id = "req-400";
    let ctx = PendingRequestContext {
        state_key: "device-xyz".to_string(),
        ttl_seconds: 300,
    };
    pending_context_port.save(request_id, &ctx).await;

    // Produce before starting the consumer — auto.offset.reset=earliest means the
    // consumer will replay this message on its first poll without needing a sleep.
    // The pending context is already in Valkey, so the consumer can process it immediately.
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("producer creation failed");

    let response = StateInitResponse {
        request_id: request_id.to_string(),
        state_jws: "init-state-jws".to_string(),
        dev_authorization_code: "dac_test_123".to_string(),
        server_jws_public_key: None,
        server_jws_kid: None,
        opaque_server_id: None,
    };
    let payload = serde_json::to_string(&response).unwrap();

    producer
        .send(
            FutureRecord::to("state-init-responses")
                .key("device-xyz")
                .payload(&payload),
            Duration::from_secs(5),
        )
        .await
        .expect("produce failed");

    let group_id = format!("it-{}", uuid::Uuid::new_v4());
    state_init_response_consumer::start(
        &bootstrap,
        &group_id,
        device_state_port.clone(),
        pending_context_port.clone(),
        cache.clone(),
    );

    // Wait for the cache to receive the response
    let result = cache
        .wait_for_response(request_id, Duration::from_secs(30))
        .await;
    assert!(result.is_some(), "Expected state-init response in cache");
    let result = result.unwrap();
    assert_eq!(result.request_id, request_id);
    assert_eq!(result.dev_authorization_code, "dac_test_123");

    // Verify device state was persisted in Valkey
    let state = device_state_port.load("device-xyz").await;
    assert_eq!(state, Some("init-state-jws".to_string()));
}

// ── BFF round-trip test ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_bff_kafka_valkey_round_trip() {
    let (_kafka, bootstrap) = start_kafka().await;
    let (_valkey, valkey_url) = start_valkey().await;
    let conn = valkey_connection_manager(&valkey_url).await;

    let device_state_port: Arc<dyn DeviceStatePort> =
        Arc::new(DeviceStateRedisAdapter::new(conn.clone()));
    let pending_context_port: Arc<dyn PendingContextPort> =
        Arc::new(PendingContextRedisAdapter::new(conn.clone()));
    let response_sink_port: Arc<dyn ResponseSinkPort> =
        Arc::new(ResponseSinkRedisAdapter::new(conn.clone(), 60));

    // Pre-save device state and pending context
    let request_id = "req-500";
    device_state_port.save("device-rt", "old-state", 300).await;
    let ctx = PendingRequestContext {
        state_key: "device-rt".to_string(),
        ttl_seconds: 300,
    };
    pending_context_port.save(request_id, &ctx).await;

    // Send a worker request via KafkaRequestSender before starting the consumer —
    // auto.offset.reset=earliest means the consumer will pick up this message on its
    // first poll without needing a sleep.
    let request_sender = KafkaRequestSender::new(&bootstrap, "v4");
    let request = HsmWorkerRequest {
        request_id: request_id.to_string(),
        state_jws: "old-state".to_string(),
        outer_request_jws: TypedJws::<OuterRequest>::new("outer-jws".to_string()),
    };
    use wallet_bff::application::port::outgoing::RequestSenderPort;
    request_sender
        .send(&request, "device-rt")
        .await
        .expect("send failed");

    // Wire ResponseService and start consumer after the request is already in Kafka
    let response_service = Arc::new(ResponseService::new(
        device_state_port.clone(),
        pending_context_port.clone(),
        response_sink_port.clone(),
    ));
    let group_id = format!("it-{}", uuid::Uuid::new_v4());
    r2ps_response_consumer::start(
        &bootstrap,
        &group_id,
        response_service as Arc<dyn ResponseUseCase>,
    );

    // Background task: consume from r2ps-requests, fabricate a HsmWorkerResponse, produce to r2ps-responses
    let bootstrap_clone = bootstrap.clone();
    tokio::spawn(async move {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &bootstrap_clone)
            .set("group.id", "rt-worker-sim")
            .set("auto.offset.reset", "earliest")
            .create()
            .expect("consumer creation failed");

        consumer.subscribe(&["r2ps-requests"]).unwrap();

        let msg = tokio::time::timeout(Duration::from_secs(30), consumer.recv())
            .await
            .expect("timeout")
            .expect("consumer error");

        let payload = msg.payload().expect("empty payload");
        let req: HsmWorkerRequest = serde_json::from_slice(payload).expect("deserialize failed");

        // Fabricate a worker response
        let response = HsmWorkerResponse {
            request_id: req.request_id,
            state_jws: Some("new-state-jws".to_string()),
            outer_response_jws: Some(TypedJws::<OuterResponse>::new("outer-response".to_string())),
            status: Status::Ok,
            error_message: None,
        };
        let resp_payload = serde_json::to_string(&response).unwrap();

        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &bootstrap_clone)
            .set("message.timeout.ms", "5000")
            .create()
            .expect("producer creation failed");

        producer
            .send(
                FutureRecord::to("r2ps-responses")
                    .key("device-rt")
                    .payload(&resp_payload),
                Duration::from_secs(5),
            )
            .await
            .expect("produce failed");
    });

    // Poll response_sink until the response appears
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(cached) = response_sink_port.load(request_id).await {
            assert_eq!(cached.request_id, request_id);
            assert_eq!(cached.status, Status::Ok);
            assert_eq!(cached.state_jws, Some("new-state-jws".to_string()));

            // Verify device state was updated in Valkey
            let state = device_state_port.load("device-rt").await;
            assert_eq!(state, Some("new-state-jws".to_string()));
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("Timeout waiting for round-trip response");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn test_ec_jwk() -> EcPublicJwk {
    EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU".to_string(),
        y: "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0".to_string(),
        kid: String::new(),
    }
}
