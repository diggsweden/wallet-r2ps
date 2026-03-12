use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::jose_port::{JosePort, JweDecryptionKey};
use crate::application::pake_port::{PakeError, PakePort, RegistrationResult};
use crate::application::service::operations::OperationResult;
use crate::application::service::worker_service::WorkerService;
use crate::application::service::worker_service::context::ResponseContext;
use crate::application::service::worker_service::error::{OuterError, UpstreamError, WorkerError};
use crate::application::service::worker_service::response::{ProcessError, ResponseBuilder};
use crate::application::session_key_spi_port::{
    ClientRepositoryError, SessionKey, SessionKeySpiPort,
};
use crate::application::{WorkerPorts, WorkerResponseError, WorkerResponseSpiPort};
use crate::domain::ServiceRequestError;
use crate::domain::value_objects::r2ps::{InnerResponse, OperationId, Status};
use crate::domain::{Curve, EcPublicJwk, HsmKey, InnerResponseData, SessionId, WorkerResponse};
use crate::infrastructure::adapters::outgoing::jose_adapter::JoseAdapter;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cryptoki::error::Error as CryptokiError;
use josekit::jws::alg::ecdsa::EcdsaJwsVerifier;
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::EncodePrivateKey;
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

struct MockWorkerResponseSpi {
    pub responses: Mutex<Vec<WorkerResponse>>,
}
impl WorkerResponseSpiPort for MockWorkerResponseSpi {
    fn send(&self, worker_response: WorkerResponse) -> Result<(), WorkerResponseError> {
        self.responses.lock().unwrap().push(worker_response);
        Ok(())
    }
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
    Arc<MockWorkerResponseSpi>,
    Arc<dyn JosePort>,
    EcdsaJwsVerifier,
) {
    let (jose, out_verifier) = setup_crypto();

    let worker_response_spi = Arc::new(MockWorkerResponseSpi {
        responses: Mutex::new(Vec::new()),
    });

    let ports = WorkerPorts {
        session_key: Arc::new(MockSessionKeySpi),
        hsm: Arc::new(MockHsmSpi),
        worker_response: worker_response_spi.clone(),
        pake: Arc::new(MockPake),
    };

    let worker_service = WorkerService::new(jose.clone(), ports);

    (worker_service, worker_response_spi, jose, out_verifier)
}

fn mock_context(request_id: &str, op_id: OperationId) -> ResponseContext {
    let secret_key = SecretKey::random(&mut rand::thread_rng());
    let public_key = secret_key.public_key();
    let encoded_point = public_key.to_encoded_point(false);
    let x = URL_SAFE_NO_PAD.encode(encoded_point.x().unwrap());
    let y = URL_SAFE_NO_PAD.encode(encoded_point.y().unwrap());

    ResponseContext {
        request_id: request_id.to_string(),
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

/// Happy path tests - exercise the building of successful responses.
/// Note that these tests verify no actual encryption.
#[cfg(test)]
mod response_encoding {
    use super::*;

    #[test]
    fn test_encode_success_session_encryption() {
        let BuilderFixture { builder, .. } = setup_builder();
        let request_id = "someRequest";
        let context = mock_context(request_id, OperationId::HsmListKeys); // HsmListKeys uses Session encryption

        let op_result = OperationResult {
            state: None,
            data: InnerResponseData::new("success_data").unwrap(),
            session_id: Some(SessionId::new()),
        };

        let response = builder.encode_response(op_result, &context).unwrap();

        assert_eq!(response.request_id, request_id);
        assert_eq!(response.status, Status::Ok);
        assert!(response.error_message.is_none());
        assert!(response.outer_response_jws.unwrap().as_str().contains(".")); // Should be a JWS
    }

    #[test]
    fn test_encode_success_device_encryption() {
        let BuilderFixture { builder, .. } = setup_builder();
        let request_id = "someRequest";
        let context = mock_context(request_id, OperationId::AuthenticateStart); // AuthenticateStart uses Device encryption

        let op_result = OperationResult {
            state: None,
            data: InnerResponseData::new("success_data").unwrap(),
            session_id: None,
        };

        let response = builder.encode_response(op_result, &context).unwrap();

        assert_eq!(response.request_id, request_id);
        assert_eq!(response.status, Status::Ok);
        assert!(response.outer_response_jws.unwrap().as_str().contains(".")); // Should be a JWS
    }
}

/// Error handling tests - exercise the building of the different error responses.
#[cfg(test)]
mod error_handling {
    use super::*;
    use crate::domain::OuterResponse;

    #[test]
    fn test_build_worker_only_error_response() {
        let BuilderFixture { builder, .. } = setup_builder();
        let request_id = "someRequest";
        let process_err = ProcessError {
            error: WorkerError::Upstream(UpstreamError::InvalidStateJws),
            context: None,
        };

        let response = builder
            .build_error_response(request_id, process_err)
            .expect("worker-only error response should build");

        assert_eq!(response.request_id, request_id);
        assert!(response.state_jws.is_none());
        assert!(response.outer_response_jws.is_none());
        assert_eq!(response.status, Status::Error);
        assert!(response.error_message.is_some());
    }

    #[test]
    fn test_build_dispatch_error_worker_visibility() {
        let BuilderFixture { builder, .. } = setup_builder();
        let request_id = "someRequest";
        let context = mock_context(request_id, OperationId::HsmGenerateKey);
        let process_err = ProcessError {
            error: WorkerError::Upstream(UpstreamError::UnknownDevice),
            context: Some(Box::new(context)),
        };

        let response = builder
            .build_error_response(request_id, process_err)
            .expect("worker-only response should build");

        assert_eq!(response.request_id, request_id);
        assert!(response.state_jws.is_none());
        assert!(response.outer_response_jws.is_none());
        assert_eq!(response.status, Status::Error);
        assert!(
            response
                .error_message
                .as_ref()
                .is_some_and(|msg| msg.contains("UnknownDevice"))
        );
    }

    #[test]
    fn test_build_dispatch_error_outer_visibility() {
        let BuilderFixture {
            builder, verifier, ..
        } = setup_builder();
        let request_id = "someRequest";
        let process_err = ProcessError {
            error: WorkerError::Outer(OuterError::UnsupportedContext),
            context: None,
        };

        let response = builder
            .build_error_response(request_id, process_err)
            .expect("outer response should build");

        // assert response is OK with an outerResponse
        assert_eq!(response.request_id, request_id);
        assert!(response.state_jws.is_none());
        assert!(response.outer_response_jws.is_some());
        assert_eq!(response.status, Status::Ok);
        assert!(response.error_message.is_none());

        // decode the outerResponse
        let jws = response.outer_response_jws.unwrap();
        let (payload, _) = josekit::jwt::decode_with_verifier(jws.as_str(), &verifier).unwrap();
        let outer_response: OuterResponse = serde_json::from_str(&payload.to_string()).unwrap();

        // assert the outerResponse is Error with an error_message
        assert_eq!(outer_response.version, 1);
        assert!(outer_response.session_id.is_none());
        assert!(outer_response.inner_jwe.is_none());
        assert_eq!(outer_response.status, Status::Error);
        assert!(
            outer_response
                .error_message
                .is_some_and(|msg| msg.contains("UnsupportedContext"))
        );
    }

    #[test]
    fn test_build_dispatch_error_inner_visibility() {
        let BuilderFixture {
            builder,
            jose,
            verifier,
        } = setup_builder();
        let request_id = "someRequest";
        let context = mock_context(request_id, OperationId::HsmGenerateKey);
        let process_err = ProcessError {
            error: WorkerError::Inner(ServiceRequestError::Unknown),
            context: Some(Box::new(context.clone())),
        };

        let response = builder
            .build_error_response(request_id, process_err)
            .expect("inner response should build");

        // assert response is OK with an outerResponse
        assert_eq!(response.request_id, request_id);
        assert!(response.state_jws.is_none());
        assert!(response.outer_response_jws.is_some());
        assert_eq!(response.status, Status::Ok);
        assert!(response.error_message.is_none());

        // decode the outerResponse
        let jws = response.outer_response_jws.unwrap();
        let (payload, _) = josekit::jwt::decode_with_verifier(jws.as_str(), &verifier).unwrap();
        let outer_response: OuterResponse = serde_json::from_str(&payload.to_string()).unwrap();

        // assert the outerResponse is Error with an error_message
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
        assert!(
            inner_response
                .error_message
                .is_some_and(|msg| msg.contains("Unknown"))
        );
    }
}

/// Orchestration tests for the flow from the execute function to a client response.
#[cfg(test)]
mod orchestration {
    use super::*;
    use crate::application::WorkerRequestUseCase;
    use crate::domain::HsmWorkerRequest;
    use crate::domain::TypedJws;

    #[test]
    fn test_execute_handles_decode_error_and_sends_response() {
        let (service, mock_response_port, _, _) = setup_worker_service();

        // Invalid request that will fail to decode due to its invalid state.
        let request = HsmWorkerRequest {
            request_id: "someRequest".to_string(),
            state_jws: TypedJws::new("invalid.state".to_string()),
            outer_request_jws: TypedJws::new("invalid.outer".to_string()),
        };

        let result = service.execute(request);

        // execute returns Ok(request_id) but the error message should be sent to the client.
        assert_eq!(result.unwrap(), "someRequest");

        // Verify the error message was sent
        let sent_responses = mock_response_port.responses.lock().unwrap();
        assert_eq!(sent_responses.len(), 1);

        let response = &sent_responses[0];
        assert_eq!(response.request_id, "someRequest");
        assert_eq!(response.status, Status::Error);
        assert!(response.state_jws.is_none());
        assert!(response.outer_response_jws.is_none());

        assert!(
            response.error_message.as_ref().is_some_and(|msg| {
                msg.contains("someRequest") && msg.contains("InvalidStateJws")
            })
        );
    }
}
