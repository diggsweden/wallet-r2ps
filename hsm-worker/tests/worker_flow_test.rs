// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use hsm_worker::application::JosePort;
use hsm_worker::application::WorkerPorts;
use hsm_worker::application::WorkerRequestUseCase;
use hsm_worker::application::port::outgoing::hsm_spi_port::{DerivedSecret, HsmSpiPort};
use hsm_worker::application::port::outgoing::pake_port::{PakeError, PakePort, RegistrationResult};
use hsm_worker::application::port::outgoing::session_state_spi_port::{
    PendingLoginState, SessionKey, SessionState, SessionStateSpiPort,
};
use hsm_worker::application::service::worker_service::WorkerService;
use hsm_worker::application::{WorkerResponseError, WorkerResponseSpiPort};
use hsm_worker::domain::value_objects::r2ps::{
    CreateKeyServiceData, Curve, DeleteKeyServiceData, InnerResponse, ListKeysResponse,
    OperationId, PakePayloadVector, PakeRequest, Status,
};
use hsm_worker::domain::{
    DeviceHsmState, DeviceKeyEntry, EcPublicJwk, HsmKey, HsmWorkerRequest, HsmWorkerResponse,
    InnerRequest, OuterRequest, OuterResponse, PasswordFile, PasswordFileEntry, SessionId,
    TypedJwe, TypedJws, WrappedPrivateKey,
};
use hsm_worker::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
use hsm_worker::infrastructure::adapters::outgoing::session_state_memory_cache::SessionStateMemoryCache;
use josekit::jwe::alg::direct::DirectJweAlgorithm;
use josekit::jwe::{ECDH_ES, JweHeader};
use josekit::jws::{ES256, JwsHeader};
use mockall::mock;
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::{EncodePrivateKey, EncodePublicKey};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Local mock for PakePort (cannot use MockPakePort — it's cfg(test)-only in lib)
// ---------------------------------------------------------------------------
mock! {
    PakeImpl {}
    impl PakePort for PakeImpl {
        fn registration_start(
            &self, request_bytes: &[u8], client_id: &str,
        ) -> Result<PakePayloadVector, PakeError>;
        fn registration_finish(
            &self, upload_bytes: &[u8],
        ) -> Result<RegistrationResult, PakeError>;
        fn authentication_start(
            &self, request_bytes: &[u8], password_file_bytes: &PasswordFileEntry, client_id: &str,
        ) -> Result<(PakePayloadVector, PendingLoginState), PakeError>;
        fn authentication_finish(
            &self, finalization_bytes: &[u8], pending_state: &PendingLoginState, client_id: &str,
        ) -> Result<SessionKey, PakeError>;
    }
}

// ---------------------------------------------------------------------------
// Local mock for HsmSpiPort
// ---------------------------------------------------------------------------
mock! {
    HsmImpl {}
    impl HsmSpiPort for HsmImpl {
        fn generate_key(&self, alias: &str, curve: &Curve) -> Result<HsmKey, Box<dyn std::error::Error>>;
        fn sign(&self, key: &HsmKey, sign_payload: &[u8]) -> Result<Vec<u8>, cryptoki::error::Error>;
        fn derive_key(
            &self,
            root_key_label: &str,
            domain_separator: &str,
        ) -> Result<DerivedSecret, cryptoki::error::Error>;
    }
}

// ---------------------------------------------------------------------------
// No-op HsmSpiPort (HSM methods are never called in the authenticate flow)
// ---------------------------------------------------------------------------
struct NoOpHsm;

impl HsmSpiPort for NoOpHsm {
    fn generate_key(&self, _: &str, _: &Curve) -> Result<HsmKey, Box<dyn std::error::Error>> {
        unimplemented!("NoOpHsm: generate_key not used in authenticate flow tests")
    }

    fn sign(&self, _: &HsmKey, _: &[u8]) -> Result<Vec<u8>, cryptoki::error::Error> {
        unimplemented!("NoOpHsm: sign not used in authenticate flow tests")
    }

    fn derive_key(&self, _: &str, _: &str) -> Result<DerivedSecret, cryptoki::error::Error> {
        unimplemented!("NoOpHsm: derive_key not used in authenticate flow tests")
    }
}

// ---------------------------------------------------------------------------
// Capturing WorkerResponseSpiPort (replaces pub(crate) MockWorkerResponseSpi)
// ---------------------------------------------------------------------------
struct CapturingResponseSink {
    responses: Mutex<Vec<HsmWorkerResponse>>,
}

impl CapturingResponseSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(vec![]),
        })
    }
}

impl WorkerResponseSpiPort for CapturingResponseSink {
    fn send(&self, r: HsmWorkerResponse) -> Result<(), WorkerResponseError> {
        self.responses.lock().unwrap().push(r);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Creates a real JoseAdapter keyed with a fresh P-256 server key.
/// Returns the adapter, a verifier for decoding signed responses, and the server's public PEM
/// (needed by the client side to encrypt inner JWEs toward the server).
fn setup_server_crypto() -> (
    Arc<JoseAdapter>,
    josekit::jws::alg::ecdsa::EcdsaJwsVerifier,
    String,
) {
    let secret = SecretKey::random(&mut rand::thread_rng());
    let pub_pem_str = secret
        .public_key()
        .to_public_key_pem(Default::default())
        .unwrap()
        .to_string();
    let jose = Arc::new(JoseAdapter::new(secret).unwrap());
    let verifier = ES256.verifier_from_pem(pub_pem_str.as_bytes()).unwrap();
    (jose, verifier, pub_pem_str)
}

/// Client-side helper: sign an OuterRequest payload as a JWT using the device's private key.
fn device_sign_jwt(payload_json: &[u8], device_private_pem_str: &str, kid: &str) -> String {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(payload_json).unwrap();
    let payload = josekit::jwt::JwtPayload::from_map(map).unwrap();
    let mut header = JwsHeader::new();
    header.set_key_id(kid);
    let signer = ES256
        .signer_from_pem(device_private_pem_str.as_bytes())
        .unwrap();
    josekit::jwt::encode_with_signer(&payload, &header, &signer).unwrap()
}

/// Client-side helper: encrypt an InnerRequest payload toward the server using ECDH-ES.
/// Kid MUST be "device" — this is the literal string the server checks to select device decryption.
fn client_encrypt_inner(payload: &[u8], server_pub_pem_str: &str) -> String {
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");
    header.set_key_id("device");
    let encrypter = ECDH_ES
        .encrypter_from_pem(server_pub_pem_str.as_bytes())
        .unwrap();
    josekit::jwe::serialize_compact(payload, &header, &encrypter).unwrap()
}

/// Client-side helper: encrypt an InnerRequest payload with a session key (dir+A256GCM, kid="session").
/// Used for post-auth operations where EncryptOption::Session is required.
fn client_encrypt_inner_session(payload: &[u8], session_key: &[u8]) -> String {
    let mut header = JweHeader::new();
    header.set_algorithm("dir");
    header.set_content_encryption("A256GCM");
    header.set_key_id("session");
    let encrypter = DirectJweAlgorithm::Dir
        .encrypter_from_bytes(session_key)
        .unwrap();
    josekit::jwe::serialize_compact(payload, &header, &encrypter).unwrap()
}

/// Client-side helper: decrypt a response inner JWE with the session key.
fn client_decrypt_inner_session(jwe_str: &str, session_key: &[u8]) -> Vec<u8> {
    let decrypter = DirectJweAlgorithm::Dir
        .decrypter_from_bytes(session_key)
        .unwrap();
    let (payload, _) = josekit::jwe::deserialize_compact(jwe_str, &decrypter).unwrap();
    payload
}

/// Client-side helper: decrypt a response inner JWE with the device private key (ECDH-ES).
fn client_decrypt_inner_device(jwe_str: &str, device_private_pem_str: &str) -> Vec<u8> {
    let decrypter = ECDH_ES
        .decrypter_from_pem(device_private_pem_str.as_bytes())
        .unwrap();
    let (payload, _) = josekit::jwe::deserialize_compact(jwe_str, &decrypter).unwrap();
    payload
}

/// Create a synthetic HsmKey for testing with a given kid.
fn make_synthetic_hsm_key(kid: &str) -> HsmKey {
    let secret = SecretKey::random(&mut rand::thread_rng());
    let ec_point = secret.public_key().to_encoded_point(false);
    HsmKey {
        wrapped_private_key: WrappedPrivateKey::new(vec![1, 2, 3, 4]),
        public_key_jwk: EcPublicJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: BASE64_URL_SAFE_NO_PAD.encode(ec_point.x().unwrap()),
            y: BASE64_URL_SAFE_NO_PAD.encode(ec_point.y().unwrap()),
            kid: kid.to_string(),
        },
        wrap_key_label: String::new(),
        created_at: chrono::Utc::now(),
    }
}

/// Create a pake mock pre-configured for a single authenticate-start → authenticate-finish sequence.
/// The session key returned by authentication_finish is always [7u8; 32].
fn make_auth_pake() -> Arc<MockPakeImpl> {
    let mut mock = MockPakeImpl::new();
    mock.expect_authentication_start()
        .once()
        .returning(|_, _, _| {
            Ok((
                PakePayloadVector::new(vec![0xAA, 0xBB]),
                PendingLoginState::new(vec![0xCC, 0xDD]),
            ))
        });
    mock.expect_authentication_finish()
        .once()
        .returning(|_, _, _| Ok(SessionKey::new(vec![7u8; 32])));
    Arc::new(mock)
}

// ---------------------------------------------------------------------------
// Shared test fixture
// ---------------------------------------------------------------------------

/// Shared test context wrapping a `WorkerService` plus all supporting objects.
///
/// The default state includes:
/// - `device_keys[0].password_files` — required for `AuthenticateStart`
/// - `device_keys[0].dev_authorization_code = Some("test-code")` — required for `RegisterStart/Finish`
struct TestFixture {
    service: WorkerService,
    response_sink: Arc<CapturingResponseSink>,
    cache: Arc<SessionStateMemoryCache>,
    #[allow(dead_code)]
    server_jose: Arc<JoseAdapter>,
    server_verifier: josekit::jws::alg::ecdsa::EcdsaJwsVerifier,
    server_pub_pem: String,
    device_private_pem: String,
    device_kid: String,
    state_jws: TypedJws<DeviceHsmState>,
}

impl TestFixture {
    /// Decode and verify a signed DeviceHsmState from a TypedJws.
    fn decode_state(&self, state_jws: &TypedJws<DeviceHsmState>) -> DeviceHsmState {
        let (payload, _) =
            josekit::jwt::decode_with_verifier(state_jws.as_str(), &self.server_verifier).unwrap();
        serde_json::from_str(&payload.to_string()).unwrap()
    }

    /// Decode and verify a signed OuterResponse from its JWS string.
    fn decode_outer_response(&self, jws: &str) -> OuterResponse {
        let (payload, _) = josekit::jwt::decode_with_verifier(jws, &self.server_verifier).unwrap();
        serde_json::from_str(&payload.to_string()).unwrap()
    }

    /// Decrypt and deserialize an InnerResponse from a session-encrypted JWE.
    fn decode_inner_session(&self, jwe_str: &str, session_key: &[u8]) -> InnerResponse {
        let bytes = client_decrypt_inner_session(jwe_str, session_key);
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Sign a DeviceHsmState and return its JWS — useful for creating custom initial states.
    #[allow(dead_code)]
    fn sign_state(&self, state: &DeviceHsmState) -> TypedJws<DeviceHsmState> {
        state.sign(&*self.server_jose).unwrap()
    }

    /// Return the most recent response from the sink (panics if sink is empty).
    fn last_response(&self) -> HsmWorkerResponse {
        self.response_sink
            .responses
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .clone()
    }

    /// Build an `HsmWorkerRequest` using this fixture's device keys and state.
    ///
    /// `session_key = Some(key)` → session-encrypted inner JWE (dir+A256GCM, kid="session")
    /// `session_key = None` → device-encrypted inner JWE (ECDH-ES, kid="device")
    fn build_request(
        &self,
        req_id: &str,
        session_id: Option<SessionId>,
        op: OperationId,
        counter: u32,
        inner_data: Option<String>,
        session_key: Option<&[u8]>,
    ) -> HsmWorkerRequest {
        let inner = InnerRequest {
            version: 1,
            request_type: op,
            request_counter: counter,
            data: inner_data,
        };
        let inner_bytes = serde_json::to_vec(&inner).unwrap();
        let inner_jwe = match session_key {
            Some(key) => client_encrypt_inner_session(&inner_bytes, key),
            None => client_encrypt_inner(&inner_bytes, &self.server_pub_pem),
        };
        let outer = OuterRequest {
            version: 1,
            session_id,
            context: "hsm".to_string(),
            server_kid: Some(self.server_jose.jws_kid().to_string()),
            inner_jwe: Some(TypedJwe::new(inner_jwe)),
        };
        let outer_jws = device_sign_jwt(
            &serde_json::to_vec(&outer).unwrap(),
            &self.device_private_pem,
            &self.device_kid,
        );
        HsmWorkerRequest {
            request_id: req_id.to_string(),
            state_jws: self.state_jws.clone(),
            outer_request_jws: TypedJws::new(outer_jws),
        }
    }
}

/// Build a `TestFixture` using the provided pake mocks.
fn make_fixture(pake: Arc<dyn PakePort>) -> TestFixture {
    make_fixture_with_hsm_keys(pake, Arc::new(NoOpHsm), vec![])
}

/// Build a `TestFixture` using the provided pake and hsm mocks.
fn make_fixture_with_hsm(
    pake: Arc<dyn PakePort>,
    hsm: Arc<dyn HsmSpiPort + Send + Sync>,
) -> TestFixture {
    make_fixture_with_hsm_keys(pake, hsm, vec![])
}

/// Build a `TestFixture` with a custom set of pre-existing HSM keys in the initial state.
/// The default state has a password file entry and dev_authorization_code="test-code".
fn make_fixture_with_hsm_keys(
    pake: Arc<dyn PakePort>,
    hsm: Arc<dyn HsmSpiPort + Send + Sync>,
    hsm_keys: Vec<HsmKey>,
) -> TestFixture {
    let (server_jose, server_verifier, server_pub_pem) = setup_server_crypto();
    let shared_cache = Arc::new(SessionStateMemoryCache::new());
    let response_sink = CapturingResponseSink::new();

    let device_secret = SecretKey::random(&mut rand::thread_rng());
    let device_private_pem = device_secret
        .to_pkcs8_pem(Default::default())
        .unwrap()
        .to_string();
    let ec_point = device_secret.public_key().to_encoded_point(false);
    let device_kid = "test-device-key".to_string();
    let device_pub_jwk = EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: BASE64_URL_SAFE_NO_PAD.encode(ec_point.x().unwrap()),
        y: BASE64_URL_SAFE_NO_PAD.encode(ec_point.y().unwrap()),
        kid: device_kid.clone(),
    };

    let state = DeviceHsmState {
        version: 1,
        device_keys: vec![DeviceKeyEntry {
            public_key: device_pub_jwk,
            password_files: vec![PasswordFileEntry {
                password_file: PasswordFile(vec![1, 2, 3]),
                opaque_domain_separator: "cloud-wallet.digg.se".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }],
            dev_authorization_code: Some("test-code".to_string()),
        }],
        hsm_keys,
    };
    let state_jws = state.sign(&*server_jose).unwrap();

    let ports = WorkerPorts {
        jose: server_jose.clone(),
        session_state: shared_cache.clone(),
        hsm,
        worker_response: response_sink.clone(),
        pake,
    };
    let service = WorkerService::new(ports, false);

    TestFixture {
        service,
        response_sink,
        cache: shared_cache,
        server_jose,
        server_verifier,
        server_pub_pem,
        device_private_pem,
        device_kid,
        state_jws,
    }
}

/// Drive authenticate-start → authenticate-finish through `fixture.service`.
///
/// Requires the fixture's pake mock to have `authentication_start` and `authentication_finish`
/// expectations configured (e.g., via `make_auth_pake()`).
///
/// Returns `(session_id, session_key_bytes)` where session_key_bytes is the [7u8; 32] value
/// returned by the mock's `authentication_finish`.
fn run_authenticate_sequence(fixture: &TestFixture) -> (SessionId, Vec<u8>) {
    let session_key_bytes = vec![7u8; 32];

    let pake_data = PakeRequest {
        authorization: None,
        purpose: None,
        data: PakePayloadVector::new(vec![0x01, 0x02]),
    };

    // authenticate-start: server creates PendingAuth session and returns a session_id
    let req1 = fixture.build_request(
        "auth-seq-1",
        None,
        OperationId::AuthenticateStart,
        1,
        Some(serde_json::to_string(&pake_data).unwrap()),
        None,
    );
    fixture.service.execute(req1).unwrap();

    // Extract session_id from the signed outer response
    let session_id = {
        let last = fixture.last_response();
        assert_eq!(last.status, Status::Ok, "authenticate-start should succeed");
        let outer =
            fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
        outer
            .session_id
            .expect("session_id must be present after authenticate-start")
    };

    // authenticate-finish: session transitions from PendingAuth → Active
    let req2 = fixture.build_request(
        "auth-seq-2",
        Some(session_id.clone()),
        OperationId::AuthenticateFinish,
        2,
        Some(serde_json::to_string(&pake_data).unwrap()),
        None,
    );
    fixture.service.execute(req2).unwrap();

    {
        let last = fixture.last_response();
        assert_eq!(
            last.status,
            Status::Ok,
            "authenticate-finish should succeed"
        );
    }

    (session_id, session_key_bytes)
}

// ---------------------------------------------------------------------------
// Pre-auth flows
// ---------------------------------------------------------------------------

/// Register-start → register-finish succeeds and updates the state_jws with a new password file.
#[test]
fn test_register_start_finish() {
    let pake_mock = {
        let mut mock = MockPakeImpl::new();
        mock.expect_registration_start()
            .once()
            .returning(|_, _| Ok(PakePayloadVector::new(vec![0xAA, 0xBB])));
        mock.expect_registration_finish().once().returning(|_| {
            Ok(RegistrationResult {
                password_file: PasswordFile(vec![0x10, 0x20, 0x30, 0x40]),
                opaque_domain_separator: "test-server".to_string(),
            })
        });
        Arc::new(mock)
    };
    let fixture = make_fixture(pake_mock);

    let pake_req = PakeRequest {
        authorization: Some("test-code".to_string()),
        purpose: None,
        data: PakePayloadVector::new(vec![0x01, 0x02]),
    };

    // RegisterStart
    let req1 = fixture.build_request(
        "reg-1",
        None,
        OperationId::RegisterStart,
        1,
        Some(serde_json::to_string(&pake_req).unwrap()),
        None,
    );
    fixture.service.execute(req1).unwrap();

    assert_eq!(
        fixture.last_response().status,
        Status::Ok,
        "register-start should succeed"
    );

    // RegisterFinish
    let req2 = fixture.build_request(
        "reg-2",
        None,
        OperationId::RegisterFinish,
        2,
        Some(serde_json::to_string(&pake_req).unwrap()),
        None,
    );
    fixture.service.execute(req2).unwrap();

    let last = fixture.last_response();
    assert_eq!(last.status, Status::Ok, "register-finish should succeed");

    // Verify the state_jws was updated: new password file is present and authorization code consumed.
    let updated_state = fixture.decode_state(last.state_jws.as_ref().unwrap());
    assert_eq!(
        updated_state.device_keys[0].password_files.len(),
        1,
        "should have exactly one password file after register-finish"
    );
    assert_eq!(
        updated_state.device_keys[0].password_files[0]
            .password_file
            .0,
        vec![0x10, 0x20, 0x30, 0x40],
        "password file bytes must match the mock RegistrationResult"
    );
    assert_eq!(
        updated_state.device_keys[0].password_files[0].opaque_domain_separator,
        "test-server"
    );
    assert!(
        updated_state.device_keys[0]
            .dev_authorization_code
            .is_none(),
        "authorization code must be consumed after register-finish"
    );
}

/// RegisterStart with a wrong authorization code must produce Status::Error.
#[test]
fn test_register_start_invalid_authorization_fails() {
    let fixture = make_fixture(Arc::new(MockPakeImpl::new()));

    let pake_req = PakeRequest {
        authorization: Some("wrong-code".to_string()),
        purpose: None,
        data: PakePayloadVector::new(vec![0x01, 0x02]),
    };

    let req = fixture.build_request(
        "reg-invalid",
        None,
        OperationId::RegisterStart,
        1,
        Some(serde_json::to_string(&pake_req).unwrap()),
        None,
    );
    fixture.service.execute(req).unwrap();

    // Inner errors (application-level) are encoded inside the device-encrypted inner JWE.
    // WorkerResponse.status is Ok at the transport layer; check InnerResponse.status instead.
    let last = fixture.last_response();
    let outer = fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
    let inner_bytes = client_decrypt_inner_device(
        outer.inner_jwe.as_ref().unwrap().as_str(),
        &fixture.device_private_pem,
    );
    let inner: InnerResponse = serde_json::from_slice(&inner_bytes).unwrap();
    assert_eq!(inner.status, Status::Error);
    // Validate RFC 9457 problem details
    let error_json: serde_json::Value =
        serde_json::from_str(inner.error_message.as_ref().unwrap()).unwrap();
    assert_eq!(
        error_json["title"], "Error processing request",
        "error title must match RFC 9457 format"
    );
    assert!(
        error_json["detail"]
            .as_str()
            .unwrap()
            .contains("InvalidAuthorizationCode"),
        "error detail must identify the failure cause"
    );
}

// ---------------------------------------------------------------------------
// Authenticate + post-auth flows
// ---------------------------------------------------------------------------

/// After a complete authenticate-start → authenticate-finish exchange, the session
/// must be `Active` in the in-memory cache, making it available for later
/// post-auth operations.
#[test]
fn test_authenticate_start_then_finish_establishes_active_session() {
    let fixture = make_fixture(make_auth_pake());
    let (session_id, _) = run_authenticate_sequence(&fixture);
    assert!(
        matches!(
            fixture.cache.get(&session_id),
            Some(SessionState::Active(_))
        ),
        "Session must be Active after authenticate-finish"
    );
}

/// EndSession removes the session from the cache.
#[test]
fn test_end_session() {
    let fixture = make_fixture(make_auth_pake());
    let (session_id, session_key) = run_authenticate_sequence(&fixture);

    let req = fixture.build_request(
        "end-session",
        Some(session_id.clone()),
        OperationId::EndSession,
        3,
        None,
        Some(&session_key),
    );
    fixture.service.execute(req).unwrap();

    let last = fixture.last_response();
    assert_eq!(last.status, Status::Ok, "end-session should succeed");

    assert!(
        fixture.cache.get(&session_id).is_none(),
        "session must be absent from cache after end-session"
    );
}

/// HsmListKeys on an empty device returns an empty key list.
#[test]
fn test_post_auth_hsm_list_keys_empty() {
    let fixture = make_fixture(make_auth_pake());
    let (session_id, session_key) = run_authenticate_sequence(&fixture);

    let req = fixture.build_request(
        "list-keys-empty",
        Some(session_id),
        OperationId::HsmListKeys,
        3,
        Some(serde_json::to_string(&serde_json::json!({})).unwrap()),
        Some(&session_key),
    );
    fixture.service.execute(req).unwrap();

    let last = fixture.last_response();
    assert_eq!(last.status, Status::Ok);

    let outer = fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
    let inner = fixture.decode_inner_session(outer.inner_jwe.unwrap().as_str(), &session_key);
    assert_eq!(inner.status, Status::Ok);

    let list: ListKeysResponse = serde_json::from_str(inner.data.unwrap().as_str()).unwrap();
    assert!(
        list.key_info.is_empty(),
        "key_info must be empty for a fresh device"
    );
}

/// HsmGenerateKey creates a new key and reflects it in the updated state_jws.
#[test]
fn test_post_auth_hsm_generate_key() {
    let synthetic_key = make_synthetic_hsm_key("gen-key-kid");
    let returned_key = synthetic_key.clone();

    let hsm = {
        let mut mock = MockHsmImpl::new();
        mock.expect_generate_key()
            .once()
            .returning(move |_, _| Ok(returned_key.clone()));
        Arc::new(mock)
    };

    let fixture = make_fixture_with_hsm(make_auth_pake(), hsm);
    let (session_id, session_key) = run_authenticate_sequence(&fixture);

    let gen_data = CreateKeyServiceData { curve: Curve::P256 };
    let req = fixture.build_request(
        "gen-key",
        Some(session_id),
        OperationId::HsmGenerateKey,
        3,
        Some(serde_json::to_string(&gen_data).unwrap()),
        Some(&session_key),
    );
    fixture.service.execute(req).unwrap();

    let last = fixture.last_response();
    assert_eq!(last.status, Status::Ok, "hsm-generate-key should succeed");

    let updated_state = fixture.decode_state(last.state_jws.as_ref().unwrap());
    assert_eq!(
        updated_state.hsm_keys.len(),
        1,
        "state must contain exactly one HSM key after generate"
    );
    assert_eq!(
        updated_state.hsm_keys[0].public_key_jwk.kid, synthetic_key.public_key_jwk.kid,
        "generated key kid must match"
    );
}

/// HsmDeleteKey removes the targeted key from the updated state_jws.
#[test]
fn test_post_auth_hsm_delete_key() {
    let initial_key = make_synthetic_hsm_key("delete-me");
    let fixture =
        make_fixture_with_hsm_keys(make_auth_pake(), Arc::new(NoOpHsm), vec![initial_key]);
    let (session_id, session_key) = run_authenticate_sequence(&fixture);

    let del_data = DeleteKeyServiceData {
        hsm_kid: "delete-me".to_string(),
    };
    let req = fixture.build_request(
        "del-key",
        Some(session_id),
        OperationId::HsmDeleteKey,
        3,
        Some(serde_json::to_string(&del_data).unwrap()),
        Some(&session_key),
    );
    fixture.service.execute(req).unwrap();

    let last = fixture.last_response();
    assert_eq!(last.status, Status::Ok, "hsm-delete-key should succeed");

    let updated_state = fixture.decode_state(last.state_jws.as_ref().unwrap());
    assert!(
        updated_state.hsm_keys.is_empty(),
        "hsm_keys must be empty after deleting the only key"
    );
}

/// HsmListKeys returns the correct key info for a device with one existing key.
#[test]
fn test_post_auth_hsm_list_keys_with_entries() {
    let existing_key = make_synthetic_hsm_key("existing-kid");
    let expected_kid = existing_key.public_key_jwk.kid.clone();
    let fixture =
        make_fixture_with_hsm_keys(make_auth_pake(), Arc::new(NoOpHsm), vec![existing_key]);
    let (session_id, session_key) = run_authenticate_sequence(&fixture);

    let req = fixture.build_request(
        "list-keys-entries",
        Some(session_id),
        OperationId::HsmListKeys,
        3,
        Some(serde_json::to_string(&serde_json::json!({})).unwrap()),
        Some(&session_key),
    );
    fixture.service.execute(req).unwrap();

    let last = fixture.last_response();
    assert_eq!(last.status, Status::Ok);

    let outer = fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
    let inner = fixture.decode_inner_session(outer.inner_jwe.unwrap().as_str(), &session_key);

    let list: ListKeysResponse = serde_json::from_str(inner.data.unwrap().as_str()).unwrap();
    assert_eq!(list.key_info.len(), 1, "should list exactly one key");
    assert_eq!(
        list.key_info[0].public_key.kid, expected_kid,
        "listed key kid must match the pre-existing key"
    );
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

/// AuthenticateFinish without a prior AuthenticateStart (no PendingAuth in cache) fails.
#[test]
fn test_authenticate_finish_without_start_fails() {
    let fixture = make_fixture(Arc::new(MockPakeImpl::new()));

    let pake_req = PakeRequest {
        authorization: None,
        purpose: None,
        data: PakePayloadVector::new(vec![0x01, 0x02]),
    };

    // Use a freshly minted session_id that is NOT in the cache
    let unknown_session_id = Some(SessionId::new());

    let req = fixture.build_request(
        "auth-finish-no-start",
        unknown_session_id,
        OperationId::AuthenticateFinish,
        1,
        Some(serde_json::to_string(&pake_req).unwrap()),
        None,
    );
    fixture.service.execute(req).unwrap();

    // Inner errors are encoded inside the device-encrypted inner JWE.
    let last = fixture.last_response();
    let outer = fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
    let inner_bytes = client_decrypt_inner_device(
        outer.inner_jwe.as_ref().unwrap().as_str(),
        &fixture.device_private_pem,
    );
    let inner: InnerResponse = serde_json::from_slice(&inner_bytes).unwrap();
    assert_eq!(
        inner.status,
        Status::Error,
        "authenticate-finish without prior start must fail"
    );
    let error_json: serde_json::Value =
        serde_json::from_str(inner.error_message.as_ref().unwrap()).unwrap();
    assert_eq!(error_json["title"], "Error processing request");
    assert!(
        error_json["detail"]
            .as_str()
            .unwrap()
            .contains("UnknownSession"),
        "error detail must contain 'UnknownSession', got: {:?}",
        error_json["detail"]
    );
}

/// A post-auth operation with an unknown session_id fails.
#[test]
fn test_post_auth_op_without_session_fails() {
    let fixture = make_fixture(Arc::new(MockPakeImpl::new()));

    let unknown_session_id = Some(SessionId::new());

    // HsmListKeys uses EncryptOption::Session; without an active session, the inner JWE
    // cannot be decrypted → the server returns Status::Error before reaching the operation.
    // We use device encryption here to reach dispatch, but that itself fails because
    // EncryptOption::Session expects a session-encrypted inner — wrong key → decryption error.
    let req = fixture.build_request(
        "post-auth-no-session",
        unknown_session_id,
        OperationId::HsmListKeys,
        1,
        Some(serde_json::to_string(&serde_json::json!({})).unwrap()),
        None, // device encryption — wrong mode for a post-auth operation
    );
    fixture.service.execute(req).unwrap();

    // The decryption fails (encryption mismatch) → WorkerError::Outer →
    // OuterResponse.status is Error; WorkerResponse.status is Ok at the transport layer.
    let last = fixture.last_response();
    let outer = fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
    assert_eq!(
        outer.status,
        Status::Error,
        "post-auth operation without session must fail"
    );
}

/// Calling EndSession twice — the second call fails because the session is already gone.
#[test]
fn test_end_session_twice_fails() {
    let fixture = make_fixture(make_auth_pake());
    let (session_id, session_key) = run_authenticate_sequence(&fixture);

    let build_end_session = |req_id: &str| {
        fixture.build_request(
            req_id,
            Some(session_id.clone()),
            OperationId::EndSession,
            3,
            None,
            Some(session_key.as_slice()),
        )
    };

    // First EndSession
    fixture.service.execute(build_end_session("end-1")).unwrap();
    assert_eq!(
        fixture.last_response().status,
        Status::Ok,
        "first end-session must succeed"
    );

    // Second end-session: session was already removed from cache, so the inner JWE
    // cannot be decrypted (no session key) and the response status is Error.
    fixture.service.execute(build_end_session("end-2")).unwrap();
    let last = fixture.last_response();
    let outer = fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
    assert_eq!(outer.status, Status::Error, "second end-session must fail");
}

/// A post-auth operation sent with device encryption (instead of session encryption) fails.
#[test]
fn test_post_auth_with_device_encryption_fails() {
    let fixture = make_fixture(make_auth_pake());
    let (session_id, _session_key) = run_authenticate_sequence(&fixture);

    // HsmListKeys requires session encryption, but we send it with device encryption
    let req = fixture.build_request(
        "post-auth-wrong-encryption",
        Some(session_id),
        OperationId::HsmListKeys,
        3,
        Some(serde_json::to_string(&serde_json::json!({})).unwrap()),
        None, // device encryption — wrong mode for HsmListKeys
    );
    fixture.service.execute(req).unwrap();

    // The inner JWE is device-encrypted, but HsmListKeys requires session encryption,
    // so decryption fails and the response status is Error.
    let last = fixture.last_response();
    let outer = fixture.decode_outer_response(last.outer_response_jws.as_ref().unwrap().as_str());
    assert_eq!(
        outer.status,
        Status::Error,
        "post-auth op with device encryption must fail (wrong encryption mode)"
    );
}
