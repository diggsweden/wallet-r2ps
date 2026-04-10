// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::producer::{BaseProducer, BaseRecord};
use rdkafka::{ClientConfig, Message};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::kafka::apache::{self, Kafka};

use hsm_worker::application::port::incoming::worker_request_use_case::WorkerRequestError;
use hsm_worker::application::port::outgoing::state_init_response_spi_port::{
    StateInitResponseError, StateInitResponseSpiPort,
};
use hsm_worker::application::{WorkerRequestUseCase, WorkerResponseSpiPort};
use hsm_worker::domain::Status;
use hsm_worker::domain::{
    EcPublicJwk, HsmWorkerRequest, HsmWorkerResponse, StateInitRequest, StateInitResponse, TypedJws,
};
use hsm_worker::infrastructure::KafkaConfig;
use hsm_worker::infrastructure::adapters::incoming::r2ps_request_kafka_message_receiver::WorkerRequestKafkaReceiver;
use hsm_worker::infrastructure::adapters::incoming::state_init_request_kafka_receiver::StateInitRequestKafkaReceiver;
use hsm_worker::infrastructure::adapters::outgoing::r2ps_response_kafka_message_sender::WorkerResponseKafkaSender;
use hsm_worker::infrastructure::adapters::outgoing::state_init_response_kafka_sender::StateInitResponseKafkaMessageSender;

// ── Container helper ─────────────────────────────────────────────────────────

async fn start_kafka() -> (ContainerAsync<Kafka>, String) {
    let container = Kafka::default().start().await.unwrap();
    let port = container
        .get_host_port_ipv4(apache::KAFKA_PORT)
        .await
        .unwrap();
    let bootstrap = format!("127.0.0.1:{}", port);
    (container, bootstrap)
}

fn make_kafka_config(bootstrap: &str) -> KafkaConfig {
    KafkaConfig {
        bootstrap_servers: bootstrap.to_string(),
        broker_address_family: "v4".to_string(),
        group_id: format!("it-{}", uuid::Uuid::new_v4()),
        group_instance_id: format!("it-{}", uuid::Uuid::new_v4()),
    }
}

// ── Test doubles ─────────────────────────────────────────────────────────────

struct CapturingWorkerUseCase {
    requests: Mutex<Vec<HsmWorkerRequest>>,
}

impl CapturingWorkerUseCase {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl WorkerRequestUseCase for CapturingWorkerUseCase {
    fn execute(&self, hsm_worker_request: HsmWorkerRequest) -> Result<String, WorkerRequestError> {
        let id = hsm_worker_request.request_id.clone();
        self.requests.lock().unwrap().push(hsm_worker_request);
        Ok(id)
    }
}

struct CapturingStateInitResponseSink {
    responses: Mutex<Vec<StateInitResponse>>,
}

impl CapturingStateInitResponseSink {
    fn new() -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
        }
    }
}

impl StateInitResponseSpiPort for CapturingStateInitResponseSink {
    fn send(&self, response: StateInitResponse) -> Result<(), StateInitResponseError> {
        self.responses.lock().unwrap().push(response);
        Ok(())
    }
}

// ── Producer tests ───────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_worker_response_sender_produces_to_kafka() {
    let (_container, bootstrap) = start_kafka().await;
    let config = make_kafka_config(&bootstrap);

    let sender = WorkerResponseKafkaSender::new(&config);
    let response = HsmWorkerResponse {
        request_id: "req-101".to_string(),
        state_jws: None,
        outer_response_jws: None,
        status: Status::Ok,
        error_message: None,
    };

    sender.send(response).expect("send failed");

    // Consume and verify
    let consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("group.id", "test-verify-worker-response")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("consumer creation failed");

    consumer.subscribe(&["r2ps-responses"]).unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(Ok(msg)) = consumer.poll(Duration::from_millis(100)) {
            let payload = msg.payload().expect("empty payload");
            let received: HsmWorkerResponse =
                serde_json::from_slice(payload).expect("deserialize failed");
            assert_eq!(received.request_id, "req-101");
            assert_eq!(received.status, Status::Ok);
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("Timeout waiting for message on r2ps-responses");
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_state_init_response_sender_produces_to_kafka() {
    let (_container, bootstrap) = start_kafka().await;
    let config = make_kafka_config(&bootstrap);

    let sender = StateInitResponseKafkaMessageSender::new(&config);
    let response = StateInitResponse {
        request_id: "req-201".to_string(),
        state_jws: TypedJws::new("test-state-jws".to_string()),
        dev_authorization_code: "dac_test".to_string(),
        server_jws_public_key: EcPublicJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: "test-x".to_string(),
            y: "test-y".to_string(),
            kid: "test-server-kid".to_string(),
        },
        server_jws_kid: "test-server-kid".to_string(),
        opaque_server_id: "test-server-id".to_string(),
    };

    sender.send(response).expect("send failed");

    let consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("group.id", "test-verify-state-init-response")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("consumer creation failed");

    consumer.subscribe(&["state-init-responses"]).unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(Ok(msg)) = consumer.poll(Duration::from_millis(100)) {
            let payload = msg.payload().expect("empty payload");
            let received: StateInitResponse =
                serde_json::from_slice(payload).expect("deserialize failed");
            assert_eq!(received.request_id, "req-201");
            assert_eq!(received.dev_authorization_code, "dac_test");
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("Timeout waiting for message on state-init-responses");
        }
    }
}

// ── Consumer tests ───────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_worker_consumer_receives_and_calls_use_case() {
    let (_container, bootstrap) = start_kafka().await;
    let config = Arc::new(make_kafka_config(&bootstrap));

    let capturing = Arc::new(CapturingWorkerUseCase::new());
    let running = Arc::new(AtomicBool::new(true));

    let receiver = WorkerRequestKafkaReceiver::new(
        capturing.clone() as Arc<dyn WorkerRequestUseCase + Send + Sync>,
        running.clone(),
    );
    let handle = receiver.start_worker_thread(config);

    // Give consumer time to subscribe and join group
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Produce an HsmWorkerRequestDto to r2ps-requests
    let producer: BaseProducer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("producer creation failed");

    let request = HsmWorkerRequest {
        request_id: "req-301".to_string(),
        state_jws: TypedJws::new("state-jws-value".to_string()),
        outer_request_jws: TypedJws::new("outer-jws-value".to_string()),
    };
    let payload = serde_json::to_string(&request).unwrap();

    producer
        .send(
            BaseRecord::to("r2ps-requests")
                .key("device-1")
                .payload(&payload),
        )
        .expect("enqueue failed");
    producer.poll(Duration::from_millis(100));

    // Wait for the capturing mock to record a call
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let captured = capturing.requests.lock().unwrap();
        if !captured.is_empty() {
            assert_eq!(captured[0].request_id, "req-301");
            break;
        }
        drop(captured);
        if std::time::Instant::now() >= deadline {
            panic!("Timeout waiting for consumer to process message");
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Shutdown
    running.store(false, Ordering::Relaxed);
    handle.join().expect("consumer thread panicked");
}

#[tokio::test]
#[ignore]
async fn test_state_init_consumer_receives_and_processes() {
    let (_container, bootstrap) = start_kafka().await;
    let config = Arc::new(make_kafka_config(&bootstrap));

    let capturing_sink = Arc::new(CapturingStateInitResponseSink::new());

    // Build a real StateInitService with a real JoseAdapter + capturing response sink
    use hsm_worker::application::service::state_init_service::StateInitService;
    use hsm_worker::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
    use p256::SecretKey;

    let secret = SecretKey::random(&mut rand::thread_rng());
    let jose = Arc::new(JoseAdapter::new(secret).unwrap());

    let state_init_service = Arc::new(StateInitService::new(
        capturing_sink.clone() as Arc<dyn StateInitResponseSpiPort + Send + Sync>,
        jose,
        "test-server-id".to_string(),
    ));

    let running = Arc::new(AtomicBool::new(true));
    let receiver = StateInitRequestKafkaReceiver::new(state_init_service, running.clone());
    let handle = receiver.start_worker_thread(config);

    // Give consumer time to subscribe
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Produce a StateInitRequest to state-init-requests
    let producer: BaseProducer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("producer creation failed");

    // Use coordinates from a real P-256 key to pass validation
    use base64::Engine;
    use base64::prelude::BASE64_URL_SAFE_NO_PAD;
    use p256::elliptic_curve::sec1::ToEncodedPoint;

    let device_secret = SecretKey::random(&mut rand::thread_rng());
    let ec_point = device_secret.public_key().to_encoded_point(false);

    let request = StateInitRequest {
        request_id: "req-401".to_string(),
        public_key: EcPublicJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: BASE64_URL_SAFE_NO_PAD.encode(ec_point.x().unwrap()),
            y: BASE64_URL_SAFE_NO_PAD.encode(ec_point.y().unwrap()),
            kid: "test-device-kid".to_string(),
        },
    };
    let payload = serde_json::to_string(&request).unwrap();

    producer
        .send(
            BaseRecord::to("state-init-requests")
                .key("device-2")
                .payload(&payload),
        )
        .expect("enqueue failed");
    producer.poll(Duration::from_millis(100));

    // Wait for the capturing sink to record a response
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let captured = capturing_sink.responses.lock().unwrap();
        if !captured.is_empty() {
            assert_eq!(captured[0].request_id, "req-401");
            assert!(
                captured[0].dev_authorization_code.starts_with("dac_"),
                "Expected dac_ prefix"
            );
            break;
        }
        drop(captured);
        if std::time::Instant::now() >= deadline {
            panic!("Timeout waiting for state-init consumer to process");
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Shutdown
    running.store(false, Ordering::Relaxed);
    handle.join().expect("state-init consumer thread panicked");
}

// ── Worker round-trip test ───────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_worker_kafka_round_trip() {
    let (_container, bootstrap) = start_kafka().await;
    let config = Arc::new(make_kafka_config(&bootstrap));

    // Build real WorkerResponseKafkaSender
    let response_sender = Arc::new(WorkerResponseKafkaSender::new(&config));

    // Build a minimal WorkerService that can handle end-session
    // (end-session doesn't need PAKE or HSM, it only needs JOSE for JWS verify/sign)
    use hsm_worker::application::WorkerPorts;
    use hsm_worker::application::port::outgoing::hsm_spi_port::HsmSpiPort;
    use hsm_worker::application::port::outgoing::pake_port::{
        PakeError, PakePort, RegistrationResult,
    };
    use hsm_worker::application::port::outgoing::session_state_spi_port::{
        PendingLoginState, SessionKey,
    };
    use hsm_worker::application::service::worker_service::WorkerService;
    use hsm_worker::domain::OperationId;
    use hsm_worker::domain::{HsmKey, InnerRequest, OuterRequest, TypedJwe};
    use hsm_worker::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
    use hsm_worker::infrastructure::adapters::outgoing::session_state_memory_cache::SessionStateMemoryCache;
    use josekit::jwe::{ECDH_ES, JweHeader};
    use josekit::jws::{ES256, JwsHeader};
    use p256::SecretKey;
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    use p256::pkcs8::{EncodePrivateKey, EncodePublicKey};

    // Generate server key pair
    use hsm_worker::application::port::outgoing::jose_port::JosePort;
    let server_secret = SecretKey::random(&mut rand::thread_rng());
    let pub_pem_str = server_secret
        .public_key()
        .to_public_key_pem(Default::default())
        .unwrap()
        .to_string();
    let jose = Arc::new(JoseAdapter::new(server_secret).unwrap());

    // No-op PAKE and HSM (not needed for list-keys on a fresh device)
    struct NoOpPake;
    impl PakePort for NoOpPake {
        fn registration_start(
            &self,
            _: &[u8],
            _: &str,
        ) -> Result<hsm_worker::domain::PakePayloadVector, PakeError> {
            unimplemented!()
        }
        fn registration_finish(&self, _: &[u8]) -> Result<RegistrationResult, PakeError> {
            unimplemented!()
        }
        fn authentication_start(
            &self,
            _: &[u8],
            _: &hsm_worker::domain::PasswordFileEntry,
            _: &str,
        ) -> Result<(hsm_worker::domain::PakePayloadVector, PendingLoginState), PakeError> {
            unimplemented!()
        }
        fn authentication_finish(
            &self,
            _: &[u8],
            _: &PendingLoginState,
            _: &str,
        ) -> Result<SessionKey, PakeError> {
            unimplemented!()
        }
    }

    struct NoOpHsm;
    impl HsmSpiPort for NoOpHsm {
        fn generate_key(
            &self,
            _: &str,
            _: &hsm_worker::domain::Curve,
        ) -> Result<HsmKey, Box<dyn std::error::Error>> {
            unimplemented!()
        }
        fn sign(&self, _: &HsmKey, _: &[u8]) -> Result<Vec<u8>, cryptoki::error::Error> {
            unimplemented!()
        }
        fn derive_key(
            &self,
            _: &str,
            _: &str,
        ) -> Result<
            hsm_worker::application::port::outgoing::hsm_spi_port::DerivedSecret,
            cryptoki::error::Error,
        > {
            unimplemented!()
        }
    }

    let session_cache = Arc::new(SessionStateMemoryCache::new());
    let ports = WorkerPorts {
        jose: jose.clone() as Arc<dyn JosePort>,
        worker_response: response_sender as Arc<dyn WorkerResponseSpiPort + Send + Sync>,
        session_state: session_cache,
        pake: Arc::new(NoOpPake),
        hsm: Arc::new(NoOpHsm),
    };
    let worker_service = Arc::new(WorkerService::new(ports, false));

    let running = Arc::new(AtomicBool::new(true));
    let receiver = WorkerRequestKafkaReceiver::new(
        worker_service as Arc<dyn WorkerRequestUseCase + Send + Sync>,
        running.clone(),
    );
    let handle = receiver.start_worker_thread(config);

    // Give consumer time to subscribe
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Build a valid HsmWorkerRequestDto for a "list-keys" operation on a fresh device
    // 1) Generate device key pair
    let device_secret = SecretKey::random(&mut rand::thread_rng());
    let device_pub = device_secret.public_key();
    let device_point = device_pub.to_encoded_point(false);

    let device_kid = "test-device-kid";
    let device_jwk = hsm_worker::domain::EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: base64::Engine::encode(
            &base64::prelude::BASE64_URL_SAFE_NO_PAD,
            device_point.x().unwrap().as_slice(),
        ),
        y: base64::Engine::encode(
            &base64::prelude::BASE64_URL_SAFE_NO_PAD,
            device_point.y().unwrap().as_slice(),
        ),
        kid: device_kid.to_string(),
    };

    // 2) Build DeviceHsmState and sign it as JWS
    let device_state = hsm_worker::domain::DeviceHsmState {
        version: 1,
        device_keys: vec![hsm_worker::domain::DeviceKeyEntry {
            public_key: device_jwk.clone(),
            password_files: vec![],
            dev_authorization_code: None,
        }],
        hsm_keys: vec![],
    };
    let state_jws: TypedJws<hsm_worker::domain::DeviceHsmState> =
        device_state.sign(jose.as_ref()).unwrap();

    // 3) Build InnerRequest for list-keys (no data needed)
    let inner_request = InnerRequest {
        version: 1,
        request_type: OperationId::HsmListKeys,
        request_counter: 1,
        data: None,
    };
    let inner_json = serde_json::to_vec(&inner_request).unwrap();

    // 4) Encrypt inner as JWE toward server using device key (ECDH-ES)
    let inner_jwe_str = {
        let mut header = JweHeader::new();
        header.set_algorithm("ECDH-ES");
        header.set_content_encryption("A256GCM");
        header.set_key_id(device_kid);
        let encrypter = ECDH_ES.encrypter_from_pem(pub_pem_str.as_bytes()).unwrap();
        josekit::jwe::serialize_compact(&inner_json, &header, &encrypter).unwrap()
    };

    // 5) Build OuterRequest and sign it with device key
    let outer_request = OuterRequest {
        version: 1,
        session_id: None,
        context: "hsm".to_string(),
        server_kid: Some(jose.jws_kid().to_string()),
        inner_jwe: Some(TypedJwe::new(inner_jwe_str)),
    };
    let outer_json = serde_json::to_vec(&outer_request).unwrap();
    let outer_request_jws = {
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&outer_json).unwrap();
        let jwt_payload = josekit::jwt::JwtPayload::from_map(map).unwrap();
        let mut header = JwsHeader::new();
        header.set_key_id(device_kid);
        let device_priv_pem_str = device_secret
            .to_pkcs8_pem(Default::default())
            .unwrap()
            .to_string();
        let signer = ES256
            .signer_from_pem(device_priv_pem_str.as_bytes())
            .unwrap();
        josekit::jwt::encode_with_signer(&jwt_payload, &header, &signer).unwrap()
    };

    let request = HsmWorkerRequest {
        request_id: "req-501".to_string(),
        state_jws,
        outer_request_jws: TypedJws::new(outer_request_jws),
    };
    let request_payload = serde_json::to_string(&request).unwrap();

    // Produce to r2ps-requests
    let producer: BaseProducer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("message.timeout.ms", "5000")
        .create()
        .expect("producer creation failed");

    producer
        .send(
            BaseRecord::to("r2ps-requests")
                .key(device_kid)
                .payload(&request_payload),
        )
        .expect("enqueue failed");
    producer.poll(Duration::from_millis(100));

    // Consume from r2ps-responses
    let resp_consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap)
        .set("group.id", "test-rt-resp-consumer")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("consumer creation failed");

    resp_consumer.subscribe(&["r2ps-responses"]).unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(Ok(msg)) = resp_consumer.poll(Duration::from_millis(100)) {
            let payload = msg.payload().expect("empty payload");
            let received: HsmWorkerResponse =
                serde_json::from_slice(payload).expect("deserialize failed");
            assert_eq!(received.request_id, "req-501");
            // The operation should succeed (list-keys on fresh device returns empty list)
            assert_eq!(
                received.status,
                Status::Ok,
                "Expected OK status, got error: {:?}",
                received.error_message
            );
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("Timeout waiting for round-trip response");
        }
    }

    // Shutdown
    running.store(false, Ordering::Relaxed);
    handle.join().expect("consumer thread panicked");
}
