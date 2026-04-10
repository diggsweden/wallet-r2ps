// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::jose_port::{JoseError, JosePort, JweDecryptionKey, MockJosePort};
use crate::application::port::outgoing::session_state_spi_port::SessionKey;
use crate::application::service::operations::InnerResponseData;
use crate::application::service::operations::OperationResult;
use crate::application::service::worker_service::context::ResponseContext;
use crate::application::service::worker_service::error::{OuterError, UpstreamError, WorkerError};
use crate::application::service::worker_service::response::{ProcessError, ResponseBuilder};
use crate::domain::ServiceRequestError;
use crate::domain::{DeviceHsmState, EcPublicJwk, SessionId};
use crate::domain::{InnerResponse, OperationId, Status};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use josekit::jws::alg::ecdsa::EcdsaJwsVerifier;
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

struct BuilderFixture {
    builder: ResponseBuilder,
    jose: Arc<dyn JosePort>,
    verifier: EcdsaJwsVerifier,
}

fn setup_builder() -> BuilderFixture {
    let (jose, verifier) = super::setup_crypto();
    let builder = ResponseBuilder::new(jose.clone());
    BuilderFixture {
        builder,
        jose,
        verifier,
    }
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
        ttl: Some(Duration::from_secs(30)),
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
            session_transition: None,
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
            session_transition: None,
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
    fn test_build_worker_internal_error_response() {
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
    fn test_build_dispatch_error_with_worker_visibility_only() {
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
    fn test_build_dispatch_error_with_outer_visibility() {
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
    fn test_build_dispatch_error_with_inner_visibility() {
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

    #[test]
    fn test_encode_response_fails_without_session_key_for_session_encrypted_op() {
        let BuilderFixture { builder, .. } = setup_builder();
        // HsmListKeys uses EncryptOption::Session; omitting session_key must fail early.
        let context = ResponseContext {
            session_key: None,
            ..mock_context("req", OperationId::HsmListKeys)
        };
        let op_result = OperationResult {
            state: None,
            data: InnerResponseData::new("x").unwrap(),
            session_id: None,
            session_transition: None,
        };

        assert!(matches!(
            builder.encode_response(op_result, &context),
            Err(WorkerError::Upstream(UpstreamError::EncodeFailed(
                "unknown_session"
            )))
        ));
    }

    // `encode_response` makes two `jws_sign` calls in a fixed order:
    //   1. outer response JWS  2. updated state JWS
    // The test guards both that order and the resulting error variant when step 2 fails.
    #[test]
    fn test_encode_response_fails_when_state_signing_fails() {
        let sign_count = Arc::new(Mutex::new(0u32));
        let sign_count_clone = sign_count.clone();
        let mut mock_jose = MockJosePort::new();
        mock_jose
            .expect_jwe_encrypt()
            .returning(|_, _| Ok("enc.jwe".to_string()));
        mock_jose.expect_jws_sign().times(2).returning(move |_| {
            let mut n = sign_count_clone.lock().unwrap();
            *n += 1;
            if *n == 1 {
                Ok("ok.jws".to_string())
            } else {
                Err(JoseError::SignError)
            }
        });
        let builder = ResponseBuilder::new(Arc::new(mock_jose));
        // mock_context provides a session_key, satisfying EncryptOption::Session for HsmListKeys.
        let context = mock_context("req", OperationId::HsmListKeys);
        let op_result = OperationResult {
            state: Some(DeviceHsmState {
                version: 1,
                device_keys: vec![],
                hsm_keys: vec![],
            }),
            data: InnerResponseData::new("x").unwrap(),
            session_id: None,
            session_transition: None,
        };

        // jwe_encrypt succeeds, first jws_sign (outer) succeeds, second jws_sign (state) fails.
        assert!(matches!(
            builder.encode_response(op_result, &context),
            Err(WorkerError::Upstream(UpstreamError::EncodeFailed(
                "state_sign_failed"
            )))
        ));
    }
}
