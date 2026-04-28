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
    DeviceStatePort, NoncePort, StateInitCorrelationPort,
};
use wallet_bff::application::service::ResponseService;
use wallet_bff::domain::{
    CachedResponse, EcPublicJwk, HsmWorkerRequest, HsmWorkerResponse, OuterRequest, OuterResponse,
    StateInitRequest, StateInitResponse, Status, TypedJws,
};
use wallet_bff::infrastructure::adapters::incoming::kafka::state_init_cache::StateInitCorrelationService;
use wallet_bff::infrastructure::adapters::incoming::kafka::{
    r2ps_response_consumer, state_init_response_consumer,
};
use wallet_bff::infrastructure::adapters::outgoing::kafka::request_sender::{
    KafkaRequestSender, KafkaStateInitSender,
};
use wallet_bff::infrastructure::adapters::outgoing::redis::device_state::DeviceStateRedisAdapter;
use wallet_bff::infrastructure::adapters::outgoing::redis::nonce::NonceRedisAdapter;

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
#[cfg_attr(not(feature = "testcontainers"), ignore)]
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

// Verifies that save() passes the TTL through to Valkey: a key saved with a 1 s
// TTL must be gone after 2 s.
#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_device_state_valkey_ttl_expiry() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = DeviceStateRedisAdapter::new(conn);

    adapter.save("ttl-key", "value", 1).await;
    assert_eq!(adapter.load("ttl-key").await, Some("value".to_string()));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(adapter.load("ttl-key").await, None);
}

// Verifies that a second save() on the same key replaces the previous value.
// The device state machine writes to the same key at each step (Registered →
// Authenticated), so silent no-op or append behaviour would be a data bug.
#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
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

// ── Nonce adapter tests ──────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_nonce_adapter_first_store_returns_true() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = NonceRedisAdapter::new(conn);

    let result = adapter.try_store("client-a", "nonce-1", 60).await;
    assert_eq!(result, Ok(true));
}

#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_nonce_adapter_duplicate_nonce_returns_false() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = NonceRedisAdapter::new(conn);

    let first = adapter.try_store("client-a", "nonce-dup", 60).await;
    let second = adapter.try_store("client-a", "nonce-dup", 60).await;
    assert_eq!(first, Ok(true));
    assert_eq!(second, Ok(false));
}

// Nonces are scoped per client: the same nonce value from two different clients
// must produce distinct Valkey keys and both be accepted.
#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_nonce_adapter_different_client_same_nonce_is_allowed() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = NonceRedisAdapter::new(conn);

    let a = adapter.try_store("client-a", "shared-nonce", 60).await;
    let b = adapter.try_store("client-b", "shared-nonce", 60).await;
    assert_eq!(a, Ok(true));
    assert_eq!(b, Ok(true));
}

// A nonce stored with TTL=1 must be accepted again after the key expires.
#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_nonce_adapter_expired_nonce_can_be_reused() {
    let (_container, url) = start_valkey().await;
    let conn = valkey_connection_manager(&url).await;
    let adapter = NonceRedisAdapter::new(conn);

    let first = adapter.try_store("client-a", "nonce-ttl", 1).await;
    assert_eq!(first, Ok(true));
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after_expiry = adapter.try_store("client-a", "nonce-ttl", 1).await;
    assert_eq!(after_expiry, Ok(true));
}

// ── Kafka producer tests ─────────────────────────────────────────────────────

#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_request_sender_produces_to_kafka() {
    let (_container, bootstrap) = start_kafka().await;

    let sender = KafkaRequestSender::new(&bootstrap, "v4", "test-responses".to_string());
    let request = HsmWorkerRequest {
        request_id: "req-100".to_string(),
        state_jws: "test-state-jws".to_string(),
        outer_request_jws: TypedJws::<OuterRequest>::new("test-outer-jws".to_string()),
        response_topic: String::new(), // overwritten by sender
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

    consumer.subscribe(&["hsm-requests"]).unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(30), consumer.recv())
        .await
        .expect("timeout waiting for message")
        .expect("consumer error");

    let payload = msg.payload().expect("empty payload");
    let received: HsmWorkerRequest = serde_json::from_slice(payload).expect("deserialize failed");
    assert_eq!(received.request_id, "req-100");
    assert_eq!(received.state_jws, "test-state-jws");
    assert_eq!(received.response_topic, "test-responses");
}

#[tokio::test]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_state_init_sender_produces_to_kafka() {
    let (_container, bootstrap) = start_kafka().await;

    let sender =
        KafkaStateInitSender::new(&bootstrap, "v4", "test-state-init-responses".to_string());
    let request = StateInitRequest {
        request_id: "req-200".to_string(),
        public_key: test_ec_jwk(),
        response_topic: String::new(), // overwritten by sender
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
    assert_eq!(received.response_topic, "test-state-init-responses");
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
    fn register_pending(
        &self,
        _: &str,
        _: &str,
        _: u64,
    ) -> tokio::sync::oneshot::Receiver<CachedResponse> {
        tokio::sync::oneshot::channel().1
    }

    fn response_ready(&self, response: HsmWorkerResponse) {
        // block_in_place so we can use the sync Mutex inside an async context
        tokio::task::block_in_place(|| {
            self.responses.blocking_lock().push(response);
        });
    }

    async fn wait_for_response(&self, _: &str, _: u64) -> Option<CachedResponse> {
        None
    }
}

#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(not(feature = "testcontainers"), ignore)]
async fn test_response_consumer_receives_and_calls_use_case() {
    let (_container, bootstrap) = start_kafka().await;

    let capturing = Arc::new(CapturingResponseUseCase::new());
    let group_id = format!("it-{}", uuid::Uuid::new_v4());
    let topic = format!("test-responses-{}", uuid::Uuid::new_v4());

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
            FutureRecord::to(&topic).key("device-1").payload(&payload),
            Duration::from_secs(5),
        )
        .await
        .expect("produce failed");

    r2ps_response_consumer::start(
        &bootstrap,
        &group_id,
        &topic,
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
async fn test_state_init_consumer_receives_and_notifies_correlation_service() {
    let (_kafka, bootstrap) = start_kafka().await;
    let (_valkey, valkey_url) = start_valkey().await;
    let conn = valkey_connection_manager(&valkey_url).await;

    let device_state_port: Arc<dyn DeviceStatePort> =
        Arc::new(DeviceStateRedisAdapter::new(conn.clone()));
    let correlation = Arc::new(StateInitCorrelationService::new(device_state_port.clone()));

    let request_id = "req-400";
    let topic = format!("test-state-init-responses-{}", uuid::Uuid::new_v4());

    // Register before producing so we don't miss the response
    let rx = correlation
        .register_pending(request_id, "device-xyz", 300)
        .await;

    // Produce before starting the consumer — auto.offset.reset=earliest means the
    // consumer will replay this message on its first poll without needing a sleep.
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
            FutureRecord::to(&topic).key("device-xyz").payload(&payload),
            Duration::from_secs(5),
        )
        .await
        .expect("produce failed");

    let group_id = format!("it-{}", uuid::Uuid::new_v4());
    state_init_response_consumer::start(
        &bootstrap,
        &group_id,
        &topic,
        correlation.clone() as Arc<dyn StateInitCorrelationPort>,
    );

    // Wait for the correlation service to deliver the response
    let result = tokio::time::timeout(Duration::from_secs(30), rx)
        .await
        .expect("timeout waiting for state-init response")
        .expect("channel closed");

    assert_eq!(result.request_id, request_id);
    assert_eq!(result.dev_authorization_code, "dac_test_123");

    // Verify device state was persisted in Valkey
    let state = device_state_port.load("device-xyz").await;
    assert_eq!(state, Some("init-state-jws".to_string()));
}

// ── BFF round-trip test ──────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_bff_kafka_round_trip() {
    let (_kafka, bootstrap) = start_kafka().await;
    let (_valkey, valkey_url) = start_valkey().await;
    let conn = valkey_connection_manager(&valkey_url).await;

    let device_state_port: Arc<dyn DeviceStatePort> =
        Arc::new(DeviceStateRedisAdapter::new(conn.clone()));

    let response_topic = format!("test-bff-responses-{}", uuid::Uuid::new_v4());

    // Pre-save device state
    let request_id = "req-500";
    device_state_port.save("device-rt", "old-state", 300).await;

    let response_service = Arc::new(ResponseService::new(
        device_state_port.clone(),
        Duration::from_secs(60),
    ));

    // Register the pending entry before sending the request
    let rx = response_service.register_pending(request_id, "device-rt", 300);

    let request_sender = KafkaRequestSender::new(&bootstrap, "v4", response_topic.clone());
    let request = HsmWorkerRequest {
        request_id: request_id.to_string(),
        state_jws: "old-state".to_string(),
        outer_request_jws: TypedJws::<OuterRequest>::new("outer-jws".to_string()),
        response_topic: String::new(), // overwritten by sender
    };
    use wallet_bff::application::port::outgoing::RequestSenderPort;
    request_sender
        .send(&request, "device-rt")
        .await
        .expect("send failed");

    // Start response consumer on the per-instance response topic
    let group_id = format!("it-{}", uuid::Uuid::new_v4());
    r2ps_response_consumer::start(
        &bootstrap,
        &group_id,
        &response_topic,
        response_service.clone() as Arc<dyn ResponseUseCase>,
    );

    // Background task: consume from hsm-requests, fabricate a HsmWorkerResponse, produce to response_topic
    let bootstrap_clone = bootstrap.clone();
    let response_topic_clone = response_topic.clone();
    tokio::spawn(async move {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &bootstrap_clone)
            .set("group.id", "rt-worker-sim")
            .set("auto.offset.reset", "earliest")
            .create()
            .expect("consumer creation failed");

        consumer.subscribe(&["hsm-requests"]).unwrap();

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
                FutureRecord::to(&response_topic_clone)
                    .key("device-rt")
                    .payload(&resp_payload),
                Duration::from_secs(5),
            )
            .await
            .expect("produce failed");
    });

    // Wait for the oneshot receiver to fire
    let cached = tokio::time::timeout(Duration::from_secs(30), rx)
        .await
        .expect("timeout waiting for round-trip response")
        .expect("channel closed");

    assert_eq!(cached.request_id, request_id);
    assert_eq!(cached.status, Status::Ok);

    // Device state saved asynchronously — give it a moment
    tokio::time::sleep(Duration::from_millis(100)).await;
    let state = device_state_port.load("device-rt").await;
    assert_eq!(state, Some("new-state-jws".to_string()));
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
