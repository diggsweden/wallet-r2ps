// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::WorkerPorts;
use crate::application::WorkerRequestUseCase;
use crate::application::hsm_spi_port::MockHsmSpiPort;
use crate::application::port::incoming::worker_request_use_case::WorkerRequestError;
use crate::application::port::outgoing::pake_port::MockPakePort;
use crate::application::port::outgoing::session_state_spi_port::PendingLoginState;
use crate::application::service::worker_service::WorkerService;
use crate::application::service::worker_service::test_utils::*;
use crate::application::session_state_spi_port::SessionKey;
use crate::domain::{HsmWorkerRequest, InnerRequest, SessionId, TypedJws};
use crate::domain::{OperationId, PakePayloadVector, Status};
use std::sync::Arc;

fn setup_worker_service() -> (WorkerService, Arc<MockWorkerResponseSpi>) {
    let (jose, _) = super::setup_crypto();
    let worker_response_spi = MockWorkerResponseSpi::new();
    let service = WorkerService::new(make_ports(jose, worker_response_spi.clone()), true);
    (service, worker_response_spi)
}

#[test]
fn test_execute_sends_error_response_when_decode_fails() {
    let (service, mock_response_port) = setup_worker_service();

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
        response
            .error_message
            .as_ref()
            .is_some_and(|msg| { msg.contains("someRequest") && msg.contains("InvalidStateJws") })
    );
}

#[test]
fn test_execute_returns_connection_error_when_response_send_fails() {
    let (jose, _) = super::setup_crypto();
    let service = WorkerService::new(make_ports(jose, Arc::new(FailingWorkerResponseSpi)), true);

    let request = HsmWorkerRequest {
        request_id: "send-fails".to_string(),
        state_jws: TypedJws::new("invalid.state".to_string()),
        outer_request_jws: TypedJws::new("invalid.outer".to_string()),
    };

    let result = service.execute(request);

    assert!(matches!(result, Err(WorkerRequestError::ConnectionError)));
}

#[test]
fn test_execute_returns_response_build_error_when_error_response_signing_fails() {
    let jose = MockJoseDeterministic::new(
        &make_state("device-kid"),
        &make_outer("unsupported-context", Some(SessionId::new())),
        &[],
        "session",
        true,
    );
    let mock_response_port = MockWorkerResponseSpi::new();
    let service = WorkerService::new(make_ports(jose, mock_response_port.clone()), true);

    let result = service.execute(make_request("response-build-fails"));

    assert!(matches!(
        result,
        Err(WorkerRequestError::ResponseBuildError)
    ));
    assert!(mock_response_port.responses.lock().unwrap().is_empty());
}

#[test]
fn test_execute_returns_internal_server_error_response_when_transition_fails() {
    let pake_request = crate::domain::PakeRequest {
        authorization: None,
        purpose: Some("login".to_string()),
        data: PakePayloadVector::new(vec![0x01, 0x02]),
    };
    let inner_bytes = serde_json::to_vec(&InnerRequest {
        version: 1,
        request_type: OperationId::AuthenticateStart,
        request_counter: 1,
        data: Some(serde_json::to_string(&pake_request).unwrap()),
    })
    .unwrap();
    // context "hsm" routes to the HSM context handler; the inner OperationId drives the actual operation
    let jose = MockJoseDeterministic::new(
        &make_state_with_password_file("device-kid"),
        &make_outer("hsm", None),
        &inner_bytes,
        "device",
        false,
    );
    let mock_response_port = MockWorkerResponseSpi::new();

    let ports = WorkerPorts {
        jose: jose.clone(),
        session_state: Arc::new(MockSessionStateTransitionErrorSpi {
            key: SessionKey::new(vec![9u8; 32]),
        }),
        hsm: Arc::new(MockHsmSpiPort::new()),
        worker_response: mock_response_port.clone(),
        pake: {
            let mut mock = MockPakePort::new();
            mock.expect_authentication_start()
                .once()
                .returning(|_, _, _| {
                    Ok((
                        PakePayloadVector::new(vec![0xAA]),
                        PendingLoginState::new(vec![0xBB]),
                    ))
                });
            Arc::new(mock)
        },
    };
    let service = WorkerService::new(ports, true);

    let result = service.execute(make_request("transition-fails"));

    // WorkerService returns Ok(request_id) when the response was successfully sent
    assert_eq!(result.unwrap(), "transition-fails");
    let sent = mock_response_port.responses.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].status, Status::Ok);
    assert!(sent[0].outer_response_jws.is_some());
    assert!(sent[0].error_message.is_none());

    let encrypted_payloads = jose.captured_inner_encrypt_payload.lock().unwrap();
    assert_eq!(encrypted_payloads.len(), 1);
    let inner_payload = String::from_utf8(encrypted_payloads[0].clone()).unwrap();
    assert!(inner_payload.contains("InternalServerError"));
    assert!(inner_payload.contains("transition-fails"));
}
