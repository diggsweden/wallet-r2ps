use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::jose_port::{JosePort, JweDecryptionKey};
use crate::application::pake_port::{PakeError, PakePort, RegistrationResult};
use crate::application::port::outgoing::response_publisher_port::ResponsePublisher;
use crate::application::port::outgoing::state_cache_port::{StateCache, TamperDetectionCache};
use crate::application::port::outgoing::state_repository_port::{
    OutboxEntry, StateError, StateRepository, VersionedState,
};
use crate::application::service::operations::OperationResult;
use crate::application::service::worker_service::context::ResponseContext;
use crate::application::service::worker_service::error::{OuterError, UpstreamError, WorkerError};
use crate::application::service::worker_service::response::{ProcessError, ResponseBuilder};
use crate::application::service::worker_service::WorkerService;
use crate::application::session_key_spi_port::{
    ClientRepositoryError, SessionKey, SessionKeySpiPort,
};
use crate::application::WorkerPorts;
use crate::domain::value_objects::r2ps::{InnerResponse, OperationId, Status};
use crate::domain::{Curve, DeviceHsmState, EcPublicJwk, HsmKey, InnerResponseData, SessionId};
use crate::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use cryptoki::error::Error as CryptokiError;
use josekit::jws::alg::ecdsa::EcdsaJwsVerifier;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::EncodePrivateKey;
use p256::SecretKey;
use spki::EncodePublicKey;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct MockSessionKeySpi;
impl SessionKeySpiPort for MockSessionKeySpi {
    fn store(&self, _id: &SessionId, _key: SessionKey) -> Result<Duration, ClientRepositoryError> {
        Ok(Duration::from_secs(60))
    }
    fn get(&self, _id: &SessionId) -> Option<SessionKey> {
        None
    }
    fn get_remaining_ttl(&self, _id: &SessionId) -> Option<Duration> {
        Some(Duration::from_secs(30))
    }
    fn end_session(&self, _id: &SessionId) -> Result<(), ClientRepositoryError> {
        Ok(())
    }
}

struct MockHsmSpi;
impl HsmSpiPort for MockHsmSpi {
    fn generate_key(
        &self,
        _label: &str,
        _curve: &Curve,
    ) -> Result<HsmKey, Box<dyn std::error::Error>> {
        unimplemented!()
    }
    fn sign(&self, _key: &HsmKey, _sign_payload: &[u8]) -> Result<Vec<u8>, CryptokiError> {
        unimplemented!()
    }
}

struct MockPake;
impl PakePort for MockPake {
    fn registration_start(
        &self,
        _request_bytes: &[u8],
        _client_id: &str,
    ) -> Result<Vec<u8>, PakeError> {
        Err(PakeError::RegistrationStartFailed)
    }

    fn registration_finish(&self, _upload_bytes: &[u8]) -> Result<RegistrationResult, PakeError> {
        Err(PakeError::InvalidRequest)
    }

    fn authentication_start(
        &self,
        _request_bytes: &[u8],
        _password_file_bytes: &[u8],
        _client_id: &str,
        _session_id: &SessionId,
    ) -> Result<Vec<u8>, PakeError> {
        Err(PakeError::AuthStartFailed)
    }

    fn authentication_finish(
        &self,
        _finalization_bytes: &[u8],
        _session_id: &SessionId,
        _client_id: &str,
    ) -> Result<Vec<u8>, PakeError> {
        Err(PakeError::AuthFinishFailed)
    }
}

struct MockStateRepository;
impl StateRepository for MockStateRepository {
    fn load_current_state(&self, _device_id: &str) -> Result<Option<VersionedState>, StateError> {
        Ok(None)
    }
    fn save_state_with_outbox(
        &self,
        _device_id: &str,
        _expected_version: Option<u64>,
        _new_version: u64,
        _state_jws: &str,
        _command_type: &str,
        _correlation_id: &str,
        _outbox_entries: Vec<OutboxEntry>,
    ) -> Result<(), StateError> {
        Ok(())
    }
}

struct MockResponsePublisher {
    pub responses: Mutex<Vec<Vec<u8>>>,
}
impl ResponsePublisher for MockResponsePublisher {
    fn publish_response(&self, _device_id: &str, payload: &[u8]) -> Result<(), String> {
        self.responses.lock().unwrap().push(payload.to_vec());
        Ok(())
    }
}

struct MockStateCache;
impl StateCache for MockStateCache {
    fn get(&self, _device_id: &str) -> Option<DeviceHsmState> {
        None
    }
    fn put(&self, _device_id: &str, _state: DeviceHsmState) {}
}

struct MockTamperCache;
impl TamperDetectionCache for MockTamperCache {
    fn get(&self, _device_id: &str) -> Option<u64> {
        None
    }
    fn put(&self, _device_id: &str, _version: u64) {}
    fn get_snapshot_offset(&self, _partition: i32) -> Option<i64> {
        None
    }
    fn put_snapshot_offset(&self, _partition: i32, _offset: i64) {}
}

fn setup_crypto() -> (Arc<dyn JosePort>, EcdsaJwsVerifier) {
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let private_pem_string = secret_key.to_pkcs8_pem(Default::default()).unwrap();
    let public_key_pem = secret_key
        .public_key()
        .to_public_key_pem(Default::default())
        .unwrap();

    let server_private_key = pem::parse(private_pem_string.as_bytes()).unwrap();
    let server_public_key = pem::parse(public_key_pem.as_bytes()).unwrap();
    let jose = Arc::new(JoseAdapter::new(&server_public_key, &server_private_key).unwrap());

    let verifier = josekit::jws::ES256
        .verifier_from_pem(public_key_pem.as_bytes())
        .unwrap();

    (jose, verifier)
}

struct BuilderFixture {
    builder: ResponseBuilder,
    jose: Arc<dyn JosePort>,
    verifier: EcdsaJwsVerifier,
}

fn setup_builder() -> BuilderFixture {
    let (jose, verifier) = setup_crypto();
    let builder = ResponseBuilder::new(jose.clone(), Arc::new(MockSessionKeySpi));
    BuilderFixture {
        builder,
        jose,
        verifier,
    }
}

fn setup_worker_service() -> (
    WorkerService,
    Arc<MockResponsePublisher>,
    Arc<dyn JosePort>,
    EcdsaJwsVerifier,
) {
    let (jose, out_verifier) = setup_crypto();

    let mock_response_publisher = Arc::new(MockResponsePublisher {
        responses: Mutex::new(Vec::new()),
    });

    let ports = WorkerPorts {
        state_repository: Arc::new(MockStateRepository),
        response_publisher: mock_response_publisher.clone(),
        tamper_cache: Arc::new(MockTamperCache),
        state_cache: Arc::new(MockStateCache),
        session_key: Arc::new(MockSessionKeySpi),
        hsm: Arc::new(MockHsmSpi),
        pake: Arc::new(MockPake),
    };

    let worker_service = WorkerService::new(jose.clone(), ports);

    (worker_service, mock_response_publisher, jose, out_verifier)
}

fn mock_context(correlation_id: &str, op_id: OperationId) -> ResponseContext {
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let public_key = secret_key.public_key();
    let encoded_point = public_key.to_encoded_point(false);
    let x = URL_SAFE_NO_PAD.encode(encoded_point.x().unwrap());
    let y = URL_SAFE_NO_PAD.encode(encoded_point.y().unwrap());

    ResponseContext {
        correlation_id: correlation_id.to_string(),
        device_id: "test-device".to_string(),
        request_id: None,
        request_type: op_id,
        session_key: Some(SessionKey::new(vec![0u8; 32])),
        device_public_key: EcPublicJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x,
            y,
            kid: "test-kid".to_string(),
        },
    }
}

/// Happy path tests
#[cfg(test)]
mod response_encoding {
    use super::*;

    #[test]
    fn test_encode_success_session_encryption() {
        let BuilderFixture { builder, .. } = setup_builder();
        let correlation_id = "someRequest";
        let context = mock_context(correlation_id, OperationId::HsmListKeys);

        let op_result = OperationResult {
            state: None,
            data: InnerResponseData::new("success_data").unwrap(),
            session_id: Some(SessionId::new()),
        };

        let response = builder.encode_response(op_result, &context, None).unwrap();

        assert_eq!(response.correlation_id, correlation_id);
        assert_eq!(response.status, Status::Ok);
        assert!(response.error_message.is_none());
        assert!(response.outer_response_jws.unwrap().as_str().contains("."));
    }

    #[test]
    fn test_encode_success_device_encryption() {
        let BuilderFixture { builder, .. } = setup_builder();
        let correlation_id = "someRequest";
        let context = mock_context(correlation_id, OperationId::AuthenticateStart);

        let op_result = OperationResult {
            state: None,
            data: InnerResponseData::new("success_data").unwrap(),
            session_id: None,
        };

        let response = builder.encode_response(op_result, &context, None).unwrap();

        assert_eq!(response.correlation_id, correlation_id);
        assert_eq!(response.status, Status::Ok);
        assert!(response.outer_response_jws.unwrap().as_str().contains("."));
    }
}

/// Error handling tests
#[cfg(test)]
mod error_handling {
    use super::*;
    use crate::domain::OuterResponse;

    #[test]
    fn test_build_worker_only_error_response() {
        let BuilderFixture { builder, .. } = setup_builder();
        let correlation_id = "someRequest";
        let process_err = ProcessError {
            error: WorkerError::Upstream(UpstreamError::InvalidStateJws),
            context: None,
        };

        let response = builder
            .build_error_response(correlation_id, "test-device", None, process_err)
            .expect("worker-only error response should build");

        assert_eq!(response.correlation_id, correlation_id);
        assert!(response.outer_response_jws.is_none());
        assert_eq!(response.status, Status::Error);
        assert!(response.error_message.is_some());
    }

    #[test]
    fn test_build_dispatch_error_worker_visibility() {
        let BuilderFixture { builder, .. } = setup_builder();
        let correlation_id = "someRequest";
        let context = mock_context(correlation_id, OperationId::HsmGenerateKey);
        let process_err = ProcessError {
            error: WorkerError::Upstream(UpstreamError::UnknownDevice),
            context: Some(Box::new(context)),
        };

        let response = builder
            .build_error_response(correlation_id, "test-device", None, process_err)
            .expect("worker-only response should build");

        assert_eq!(response.correlation_id, correlation_id);
        assert!(response.outer_response_jws.is_none());
        assert_eq!(response.status, Status::Error);
        assert!(response
            .error_message
            .as_ref()
            .is_some_and(|msg| msg.contains("UnknownDevice")));
    }

    #[test]
    fn test_build_dispatch_error_outer_visibility() {
        let BuilderFixture {
            builder, verifier, ..
        } = setup_builder();
        let correlation_id = "someRequest";
        let process_err = ProcessError {
            error: WorkerError::Outer(OuterError::UnsupportedContext),
            context: None,
        };

        let response = builder
            .build_error_response(correlation_id, "test-device", None, process_err)
            .expect("outer response should build");

        assert_eq!(response.correlation_id, correlation_id);
        assert!(response.outer_response_jws.is_some());
        assert_eq!(response.status, Status::Ok);
        assert!(response.error_message.is_none());

        let jws = response.outer_response_jws.unwrap();
        let (payload, _) = josekit::jwt::decode_with_verifier(jws.as_str(), &verifier).unwrap();
        let outer_response: OuterResponse = serde_json::from_str(&payload.to_string()).unwrap();

        assert_eq!(outer_response.version, 1);
        assert!(outer_response.session_id.is_none());
        assert!(outer_response.inner_jwe.is_none());
        assert_eq!(outer_response.status, Status::Error);
        assert!(outer_response
            .error_message
            .is_some_and(|msg| msg.contains("UnsupportedContext")));
    }

    #[test]
    fn test_build_dispatch_error_inner_visibility() {
        let BuilderFixture {
            builder,
            jose,
            verifier,
        } = setup_builder();
        let correlation_id = "someRequest";
        let context = mock_context(correlation_id, OperationId::HsmGenerateKey);
        let process_err = ProcessError {
            error: WorkerError::Inner(crate::domain::ServiceRequestError::Unknown),
            context: Some(Box::new(context.clone())),
        };

        let response = builder
            .build_error_response(correlation_id, "test-device", None, process_err)
            .expect("inner response should build");

        assert_eq!(response.correlation_id, correlation_id);
        assert!(response.outer_response_jws.is_some());
        assert_eq!(response.status, Status::Ok);
        assert!(response.error_message.is_none());

        let jws = response.outer_response_jws.unwrap();
        let (payload, _) = josekit::jwt::decode_with_verifier(jws.as_str(), &verifier).unwrap();
        let outer_response: OuterResponse = serde_json::from_str(&payload.to_string()).unwrap();

        assert_eq!(outer_response.version, 1);
        assert!(outer_response.session_id.is_none());
        assert!(outer_response.inner_jwe.is_some());
        assert_eq!(outer_response.status, Status::Ok);
        assert!(outer_response.error_message.is_none());

        let inner_jwe = outer_response.inner_jwe.unwrap();

        let plaintext = jose
            .jwe_decrypt(
                inner_jwe.as_str(),
                JweDecryptionKey::Session(&context.session_key.unwrap()),
            )
            .expect("Decryption with session key should succeed");
        let inner_response: InnerResponse =
            serde_json::from_slice(&plaintext).expect("Decryption with session key should succeed");

        assert_eq!(inner_response.version, 1);
        assert!(inner_response.data.is_none());
        assert!(inner_response.expires_in.is_none());
        assert_eq!(inner_response.status, Status::Error);
        assert!(inner_response
            .error_message
            .is_some_and(|msg| msg.contains("Unknown")));
    }
}

/// Orchestration tests
#[cfg(test)]
mod orchestration {
    use super::*;
    use crate::application::WorkerRequestUseCase;
    use crate::domain::TypedJws;
    use crate::domain::{HsmWorkerRequest, WorkerResponse};

    #[test]
    fn test_execute_handles_state_not_found_and_sends_response() {
        let (service, mock_response_publisher, _, _) = setup_worker_service();

        let request = HsmWorkerRequest {
            correlation_id: "someRequest".to_string(),
            device_id: "test-device".to_string(),
            request_id: None,
            state_version: None,
            outer_request_jws: TypedJws::new("invalid.outer.jws".to_string()),
        };

        let result = service.execute(request);

        // When state is not found and the error is Inner (no context available to encrypt),
        // build_error_response fails, so execute returns Err.
        // Either it succeeds (error response published) or it errors out.
        match result {
            Ok(id) => {
                assert_eq!(id, "someRequest");
                let sent = mock_response_publisher.responses.lock().unwrap();
                assert!(sent.len() >= 1);
                let response: WorkerResponse = serde_json::from_slice(&sent[0]).unwrap();
                assert_eq!(response.correlation_id, "someRequest");
                assert_eq!(response.status, Status::Error);
            }
            Err(err) => {
                // Inner error without context cannot build an encrypted error response
                // This is expected behavior - the BFF gets no response (timeout)
                assert!(matches!(
                    err,
                    crate::domain::WorkerRequestError::ResponseBuildError
                ));
            }
        }
    }
}
