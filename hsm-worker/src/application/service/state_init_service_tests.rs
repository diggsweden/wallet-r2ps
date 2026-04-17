// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::jose_port::{JoseError, MockJosePort};
use crate::application::port::outgoing::state_init_response_spi_port::{
    StateInitResponseError, StateInitResponseSpiPort,
};
use crate::application::service::state_init_service::{StateInitError, StateInitService};
use crate::domain::{EcPublicJwk, StateInitRequest, StateInitResponse};
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

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[test]
fn test_valid_initialization_pipeline() {
    let mock_spi = Arc::new(MockStateInitResponseSpi {
        responses: Mutex::new(Vec::new()),
        fail: false,
    });
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
    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(mock_jose),
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        public_key: create_valid_jwk(),
        response_topic: "test-topic".to_string(),
    };

    // Execute the service
    let result = service.initialize(request);

    // Verify the response ID matches
    assert_eq!(result.unwrap(), "test-req-123");

    // Verify the response was constructed and sent to Kafka successfully
    let responses = mock_spi.responses.lock().unwrap();
    assert_eq!(responses.len(), 1);

    let response = &responses[0];
    assert_eq!(response.request_id, "test-req-123");
    assert!(response.dev_authorization_code.starts_with("dac_"));
    assert_eq!(response.state_jws.as_str(), "mocked.jws.signature");
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
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        public_key: create_valid_jwk(),
        response_topic: "test-topic".to_string(),
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
    let service = StateInitService::new(
        mock_spi.clone(),
        Arc::new(mock_jose),
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        public_key: create_valid_jwk(),
        response_topic: "test-topic".to_string(),
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
        "mock-opaque-id".to_string(),
    );

    let request = StateInitRequest {
        request_id: "test-req-123".to_string(),
        public_key: EcPublicJwk {
            kty: kty.to_string(),
            crv: crv.to_string(),
            x: x.to_string(),
            y: y.to_string(),
            kid: kid.to_string(),
        },
        response_topic: "test-topic".to_string(),
    };

    let result = service.initialize(request);

    // Verify that the operation fast-fails cleanly
    assert!(matches!(result, Err(StateInitError::InvalidJwk)));

    // Verify that no payload was signed or sent to downstream systems
    let responses = mock_spi.responses.lock().unwrap();
    assert_eq!(responses.len(), 0);
}
