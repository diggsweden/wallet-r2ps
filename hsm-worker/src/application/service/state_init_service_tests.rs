// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::hsm_spi_port::MockHsmSpiPort;
use crate::application::port::outgoing::jose_port::{JoseError, MockJosePort};
use crate::application::port::outgoing::state_init_response_spi_port::{
    StateInitResponseError, StateInitResponseSpiPort,
};
use crate::application::service::state_init_service::{StateInitError, StateInitService};
use crate::domain::{
    Curve, EcPublicJwk, HsmKey, StateInitRequest, StateInitResponse, WrappedPrivateKey,
};
use std::sync::{Arc, Mutex};

// -----------------------------------------------------------------------------
// Mocks
// -----------------------------------------------------------------------------

struct MockStateInitResponseSpi {
    pub responses: Mutex<Vec<StateInitResponse>>,
    pub fail: bool,
}

impl StateInitResponseSpiPort for MockStateInitResponseSpi {
    fn send(
        &self,
        response: StateInitResponse,
        _response_topic: &str,
    ) -> Result<(), StateInitResponseError> {
        if self.fail {
            return Err(StateInitResponseError::ConnectionError);
        }
        self.responses.lock().unwrap().push(response);
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn create_valid_jwk() -> EcPublicJwk {
    EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: "some_x_coord".to_string(),
        y: "some_y_coord".to_string(),
        kid: "test-kid-123".to_string(),
    }
}

fn create_valid_jwe_jwk() -> EcPublicJwk {
    EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: "some_jwe_x_coord".to_string(),
        y: "some_jwe_y_coord".to_string(),
        kid: "test-jwe-kid-456".to_string(),
    }
}

fn make_mock_jose() -> MockJosePort {
    let mut mock_jose = MockJosePort::new();
    mock_jose
        .expect_jws_sign()
        .returning(|_| Ok("mocked.jws.signature".to_string()));
    mock_jose
        .expect_jws_public_key()
        .return_const(create_valid_jwk());
    mock_jose
        .expect_jws_kid()
        .return_const("mock-kid".to_string());
    mock_jose
        .expect_jwe_public_key()
        .return_const(create_valid_jwe_jwk());
    mock_jose
        .expect_jwe_kid()
        .return_const("test-jwe-kid-456".to_string());
    mock_jose
}

fn make_hsm_key(kid: &str) -> HsmKey {
    HsmKey {
        wrapped_private_key: WrappedPrivateKey::new(vec![1, 2, 3]),
        public_key_jwk: EcPublicJwk {
            kty: "EC".to_string(),
            crv: "P-256".to_string(),
            x: "gen_x".to_string(),
            y: "gen_y".to_string(),
            kid: kid.to_string(),
        },
        wrap_key_label: "test-wrap-key".to_string(),
        created_at: chrono::Utc::now(),
    }
}

fn make_succeeding_hsm() -> MockHsmSpiPort {
    let mut mock_hsm = MockHsmSpiPort::new();
    mock_hsm
        .expect_generate_key()
        .once()
        .returning(|_, _| Ok(make_hsm_key("initial-hsm-kid")));
    mock_hsm
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[test]
fn test_valid_initialization_pipeline() {
    let mock_spi = Arc::new(MockStateInitResponseSpi {
        responses: Mutex::new(Vec::new()),
        fail: false,
    });
    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(make_mock_jose()),
        Arc::new(make_succeeding_hsm()),
        "wallet-hsm-key".to_string(),
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        client_jws_public_key: create_valid_jwk(),
        client_jwe_public_key: create_valid_jwe_jwk(),
        response_topic: "test-topic".to_string(),
        initial_key_curve: Curve::P256,
    };

    let result = service.initialize(request);

    assert_eq!(result.unwrap(), "test-req-123");

    let responses = mock_spi.responses.lock().unwrap();
    assert_eq!(responses.len(), 1);

    let response = &responses[0];
    assert_eq!(response.request_id, "test-req-123");
    assert!(response.dev_authorization_code.starts_with("dac_"));
    assert_eq!(response.state_jws.as_str(), "mocked.jws.signature");
    assert_eq!(response.initial_hsm_key.kid, "initial-hsm-kid");
    assert_eq!(response.initial_hsm_key.crv, "P-256");
    assert_eq!(response.server_jws_kid, "mock-kid");
    assert_eq!(response.server_jwe_kid, "test-jwe-kid-456");
    assert_eq!(response.server_jwe_public_key.kid, "test-jwe-kid-456");
    assert_ne!(
        response.server_jws_public_key.kid,
        response.server_jwe_public_key.kid
    );
}

#[test]
fn test_initialization_fails_on_hsm_key_generation_error() {
    let mock_spi = Arc::new(MockStateInitResponseSpi {
        responses: Mutex::new(Vec::new()),
        fail: false,
    });

    let mut mock_hsm = MockHsmSpiPort::new();
    mock_hsm
        .expect_generate_key()
        .once()
        .returning(|_, _| Err("HSM failure".into()));

    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(make_mock_jose()),
        Arc::new(mock_hsm),
        "wallet-hsm-key".to_string(),
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-fail".to_string(),
        client_jws_public_key: create_valid_jwk(),
        client_jwe_public_key: create_valid_jwe_jwk(),
        response_topic: "test-topic".to_string(),
        initial_key_curve: Curve::P256,
    };

    let result = service.initialize(request);

    assert!(matches!(result, Err(StateInitError::HsmKeyGenerationError)));

    // Verify no response was sent
    assert_eq!(mock_spi.responses.lock().unwrap().len(), 0);
}

#[test]
fn test_initialization_fails_on_signing_error() {
    let mock_spi = Arc::new(MockStateInitResponseSpi {
        responses: Mutex::new(Vec::new()),
        fail: false,
    });
    // Simulate a failure in the Jose signing engine
    let mut mock_jose = MockJosePort::new();
    mock_jose
        .expect_jws_sign()
        .returning(|_| Err(JoseError::SignError));
    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(mock_jose),
        Arc::new(make_succeeding_hsm()),
        "wallet-hsm-key".to_string(),
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        client_jws_public_key: create_valid_jwk(),
        client_jwe_public_key: create_valid_jwe_jwk(),
        response_topic: "test-topic".to_string(),
        initial_key_curve: Curve::P256,
    };

    let result = service.initialize(request);

    // Verify it maps to SigningError
    assert!(matches!(result, Err(StateInitError::SigningError)));

    // Verify no response was ever sent to Kafka
    assert_eq!(mock_spi.responses.lock().unwrap().len(), 0);
}

#[test]
fn test_initialization_fails_on_spi_send_error() {
    // Simulate a failure in the Kafka response port (e.g. connection timeout)
    let mock_spi = Arc::new(MockStateInitResponseSpi {
        responses: Mutex::new(Vec::new()),
        fail: true,
    });
    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(make_mock_jose()),
        Arc::new(make_succeeding_hsm()),
        "wallet-hsm-key".to_string(),
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        client_jws_public_key: create_valid_jwk(),
        client_jwe_public_key: create_valid_jwe_jwk(),
        response_topic: "test-topic".to_string(),
        initial_key_curve: Curve::P256,
    };

    let result = service.initialize(request);

    // Verify it maps to SendError
    assert!(matches!(result, Err(StateInitError::SendError)));
}

use rstest::rstest;

#[rstest]
#[case::invalid_kty("RSA", "P-256", "x", "y", "kid")]
#[case::invalid_crv("EC", "P-384", "x", "y", "kid")]
#[case::missing_x("EC", "P-256", "", "y", "kid")]
#[case::missing_y("EC", "P-256", "x", "", "kid")]
#[case::missing_kid("EC", "P-256", "x", "y", "")]
fn test_strict_jwk_validation_rejection(
    #[case] kty: &str,
    #[case] crv: &str,
    #[case] x: &str,
    #[case] y: &str,
    #[case] kid: &str,
) {
    let mock_spi = Arc::new(MockStateInitResponseSpi {
        responses: Mutex::new(Vec::new()),
        fail: false,
    });
    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(MockJosePort::new()),
        Arc::new(MockHsmSpiPort::new()),
        "wallet-hsm-key".to_string(),
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        client_jws_public_key: EcPublicJwk {
            kty: kty.to_string(),
            crv: crv.to_string(),
            x: x.to_string(),
            y: y.to_string(),
            kid: kid.to_string(),
        },
        client_jwe_public_key: create_valid_jwk(),
        response_topic: "test-topic".to_string(),
        initial_key_curve: Curve::P256,
    };

    let result = service.initialize(request);

    // Verify that the operation fast-fails cleanly
    assert!(matches!(result, Err(StateInitError::InvalidJwk)));

    // Verify that no payload was signed or sent to downstream systems
    let responses = mock_spi.responses.lock().unwrap();
    assert_eq!(responses.len(), 0);
}

#[test]
fn test_initialization_rejects_identical_client_jws_and_jwe_keys() {
    let mock_spi = Arc::new(MockStateInitResponseSpi {
        responses: Mutex::new(Vec::new()),
        fail: false,
    });
    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(MockJosePort::new()),
        Arc::new(MockHsmSpiPort::new()),
        "wallet-hsm-key".to_string(),
        "mock-opaque-id".to_string(),
    );

    // Same key material with a different kid — must still be rejected,
    // since kid is client-supplied and proves nothing about the key.
    let mut jwe_key = create_valid_jwk();
    jwe_key.kid = "different-kid".to_string();

    let request = StateInitRequest {
        request_id: "test-req-same-key".to_string(),
        client_jws_public_key: create_valid_jwk(),
        client_jwe_public_key: jwe_key,
        response_topic: "test-topic".to_string(),
        initial_key_curve: Curve::P256,
    };

    let result = service.initialize(request);

    assert!(matches!(result, Err(StateInitError::InvalidJwk)));
    assert_eq!(mock_spi.responses.lock().unwrap().len(), 0);
}
