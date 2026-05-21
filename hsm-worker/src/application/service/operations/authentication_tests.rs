// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::pake_port::{MockPakePort, RegistrationResult};
use crate::application::port::outgoing::session_state_spi_port::{
    OngoingOperation, PendingAuthData, PendingLoginState, SessionData, SessionKey, SessionState,
};
use crate::application::service::operations::authentication::{
    AuthenticateFinishOperation, AuthenticateStartOperation, PinChangeFinishOperation,
    PinChangeStartOperation, RegisterFinishOperation, RegisterStartOperation,
};
use crate::application::service::operations::{
    OperationContext, ServiceOperation, SessionTransition,
};
use crate::domain::{
    DeviceHsmState, DeviceKeyEntry, EcPublicJwk, InnerRequest, OperationId, OuterRequest,
    PakePayloadVector, PakeRequest, ServiceRequestError, SessionId,
};
use rstest::rstest;
use std::sync::Arc;

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn state_without_password_file() -> DeviceHsmState {
    DeviceHsmState {
        version: 1,
        device_keys: vec![DeviceKeyEntry {
            public_key: EcPublicJwk {
                kty: "EC".to_string(),
                crv: "P-256".to_string(),
                x: "x".to_string(),
                y: "y".to_string(),
                kid: "device-key-1".to_string(),
            },
            password_files: vec![],
            dev_authorization_code: None,
        }],
        hsm_keys: vec![],
    }
}

fn pake_inner_request(op: OperationId) -> InnerRequest {
    let pake_req = PakeRequest {
        authorization: None,
        purpose: None,
        data: PakePayloadVector::new(vec![1, 2, 3]),
    };
    InnerRequest {
        version: 1,
        request_type: op,
        data: Some(serde_json::to_string(&pake_req).unwrap()),
    }
}

fn base_context(state: DeviceHsmState, inner_request: InnerRequest) -> OperationContext {
    OperationContext {
        request_id: "test-request".to_string(),
        state,
        outer_request: OuterRequest {
            version: 1,
            session_id: None,
            inner_jwe: None,
            server_kid: None,
            nonce: "test-nonce".to_string(),
        },
        inner_request,
        session_id: None,
        device_kid: "device-key-1".to_string(),
        session_state: None,
    }
}

// -----------------------------------------------------------------------------
// AuthenticateStartOperation
// -----------------------------------------------------------------------------

#[test]
fn test_authenticate_start_unknown_client_fails() {
    let op = AuthenticateStartOperation::new(Arc::new(MockPakePort::new()));

    let context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::AuthenticateStart),
    );

    let result = op.execute(context);

    assert!(matches!(result, Err(ServiceRequestError::UnknownClient)));
}

// -----------------------------------------------------------------------------
// AuthenticateFinishOperation
// -----------------------------------------------------------------------------

#[test]
fn test_authenticate_finish_no_session_id_fails() {
    let op = AuthenticateFinishOperation::new(Arc::new(MockPakePort::new()));

    let mut context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::AuthenticateFinish),
    );
    context.session_id = None;
    context.session_state = Some(SessionState::PendingAuth(PendingAuthData {
        server_login: PendingLoginState::new(vec![1]),
        purpose: None,
    }));

    let result = op.execute(context);

    assert!(matches!(result, Err(ServiceRequestError::UnknownSession)));
}

#[test]
fn test_authenticate_finish_wrong_state_active_fails() {
    let op = AuthenticateFinishOperation::new(Arc::new(MockPakePort::new()));

    let mut context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::AuthenticateFinish),
    );
    context.session_id = Some(SessionId::new());
    context.session_state = Some(SessionState::Active(SessionData {
        session_key: SessionKey::new(vec![1]),
        purpose: None,
        operation: None,
        has_performed_hsm_operation: false,
    }));

    let result = op.execute(context);

    assert!(matches!(result, Err(ServiceRequestError::UnknownSession)));
}

#[test]
fn test_authenticate_finish_no_session_state_fails() {
    let op = AuthenticateFinishOperation::new(Arc::new(MockPakePort::new()));

    let mut context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::AuthenticateFinish),
    );
    context.session_id = Some(SessionId::new());
    context.session_state = None;

    let result = op.execute(context);

    assert!(matches!(result, Err(ServiceRequestError::UnknownSession)));
}

// -----------------------------------------------------------------------------
// RegisterFinishOperation
// -----------------------------------------------------------------------------

fn state_with_auth_code(code: &str) -> DeviceHsmState {
    DeviceHsmState {
        version: 1,
        device_keys: vec![DeviceKeyEntry {
            public_key: EcPublicJwk {
                kty: "EC".to_string(),
                crv: "P-256".to_string(),
                x: "x".to_string(),
                y: "y".to_string(),
                kid: "device-key-1".to_string(),
            },
            password_files: vec![],
            dev_authorization_code: Some(code.to_string()),
        }],
        hsm_keys: vec![],
    }
}

fn register_start_inner_request(auth_code: Option<&str>) -> InnerRequest {
    let pake_req = PakeRequest {
        authorization: auth_code.map(|s| s.to_string()),
        purpose: None,
        data: PakePayloadVector::new(vec![1, 2, 3]),
    };
    InnerRequest {
        version: 1,
        request_type: OperationId::RegisterStart,
        data: Some(serde_json::to_string(&pake_req).unwrap()),
    }
}

fn register_finish_inner_request(auth_code: &str) -> InnerRequest {
    let pake_req = PakeRequest {
        authorization: Some(auth_code.to_string()),
        purpose: None,
        data: PakePayloadVector::new(vec![1, 2, 3]),
    };
    InnerRequest {
        version: 1,
        request_type: OperationId::RegisterFinish,
        data: Some(serde_json::to_string(&pake_req).unwrap()),
    }
}

#[test]
fn register_finish_consumes_auth_code_and_replaces_password_file() {
    const AUTH_CODE: &str = "secret-code";
    const NEW_PF_BYTES: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF];
    const OPAQUE_DOMAIN_SEP: &str = "rk-202501_opaque-202501";

    let mut mock = MockPakePort::new();
    mock.expect_registration_finish().once().returning(|_| {
        Ok(RegistrationResult {
            password_file: crate::domain::PasswordFile(NEW_PF_BYTES.to_vec()),
            opaque_domain_separator: OPAQUE_DOMAIN_SEP.to_string(),
        })
    });
    let op = RegisterFinishOperation::new(Arc::new(mock));

    let context = base_context(
        state_with_auth_code(AUTH_CODE),
        register_finish_inner_request(AUTH_CODE),
    );

    let result = op.execute(context).expect("first call should succeed");

    let new_state = result.state.expect("operation must return updated state");
    let device_key = new_state.find_device_key("device-key-1").unwrap();

    assert!(
        device_key.dev_authorization_code.is_none(),
        "auth code must be cleared after use"
    );
    assert_eq!(device_key.password_files.len(), 1);
    assert_eq!(device_key.password_files[0].password_file.0, NEW_PF_BYTES);
    assert_eq!(
        device_key.password_files[0].opaque_domain_separator,
        OPAQUE_DOMAIN_SEP
    );
}

#[test]
fn register_finish_reuse_of_consumed_auth_code_fails() {
    const AUTH_CODE: &str = "secret-code";

    let mut mock = MockPakePort::new();
    mock.expect_registration_finish().once().returning(|_| {
        Ok(RegistrationResult {
            password_file: crate::domain::PasswordFile(vec![0x01]),
            opaque_domain_separator: "rk-202501_opaque-202501".to_string(),
        })
    });
    let op = RegisterFinishOperation::new(Arc::new(mock));

    // First call: succeeds and consumes the code
    let context = base_context(
        state_with_auth_code(AUTH_CODE),
        register_finish_inner_request(AUTH_CODE),
    );
    let first = op.execute(context).expect("first call should succeed");

    // Second call: reuse the same auth code against the updated state (code is now None)
    let spent_state = first.state.unwrap();
    let context2 = base_context(spent_state, register_finish_inner_request(AUTH_CODE));
    let result = op.execute(context2);

    assert!(
        matches!(result, Err(ServiceRequestError::InvalidAuthorizationCode)),
        "reused auth code must be rejected"
    );
}

// -----------------------------------------------------------------------------
// RegisterStartOperation
// -----------------------------------------------------------------------------

/// Both missing and wrong auth codes must be rejected with the same error variant.
#[rstest]
#[case(None)]
#[case(Some("wrong"))]
fn register_start_auth_code_rejected(#[case] authorization: Option<&'static str>) {
    let op = RegisterStartOperation::new(Arc::new(MockPakePort::new()));
    let context = base_context(
        state_with_auth_code("abc123"),
        register_start_inner_request(authorization),
    );

    let result = op.execute(context);

    assert!(
        matches!(result, Err(ServiceRequestError::InvalidAuthorizationCode)),
        "expected InvalidAuthorizationCode for authorization: {authorization:?}"
    );
}

#[test]
fn register_start_unknown_key_fails() {
    let op = RegisterStartOperation::new(Arc::new(MockPakePort::new()));
    let state = DeviceHsmState {
        version: 1,
        device_keys: vec![],
        hsm_keys: vec![],
    };
    // authorization must be Some to reach the key-lookup branch
    let context = base_context(state, register_start_inner_request(Some("any-code")));

    let result = op.execute(context);

    assert!(matches!(result, Err(ServiceRequestError::UnknownKey)));
}

#[test]
fn register_start_matching_auth_code_succeeds() {
    const AUTH_CODE: &str = "abc123";

    let mut mock = MockPakePort::new();
    mock.expect_registration_start()
        .once()
        .returning(|_, _| Ok(PakePayloadVector::new(vec![4, 5, 6])));
    let op = RegisterStartOperation::new(Arc::new(mock));

    let context = base_context(
        state_with_auth_code(AUTH_CODE),
        register_start_inner_request(Some(AUTH_CODE)),
    );

    let result = op.execute(context);

    assert!(result.is_ok(), "expected Ok");
    assert!(
        result.unwrap().session_transition.is_none(),
        "RegisterStart must not produce a session transition"
    );
}

// -----------------------------------------------------------------------------
// PinChangeFinishOperation
// -----------------------------------------------------------------------------

#[test]
fn pin_change_finish_replaces_password_file_and_ends_session() {
    const NEW_PF_BYTES: &[u8] = &[0xCA, 0xFE, 0xBA, 0xBE];
    const OPAQUE_DOMAIN_SEP: &str = "rk-202501_opaque-202501";

    let mut mock = MockPakePort::new();
    mock.expect_registration_finish().once().returning(|_| {
        Ok(RegistrationResult {
            password_file: crate::domain::PasswordFile(vec![0xCA, 0xFE, 0xBA, 0xBE]),
            opaque_domain_separator: "rk-202501_opaque-202501".to_string(),
        })
    });
    let op = PinChangeFinishOperation::new(Arc::new(mock));
    let mut context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::ChangePinFinish),
    );
    context.session_state = Some(SessionState::Active(SessionData {
        session_key: SessionKey::new(vec![1]),
        purpose: None,
        operation: Some(OngoingOperation::ChangingPin),
        has_performed_hsm_operation: false,
    }));

    let result = op.execute(context).expect("should succeed");

    assert!(
        matches!(result.session_transition, Some(SessionTransition::End)),
        "session must be ended after pin change"
    );
    let new_state = result.state.expect("must return updated state");
    let device_key = new_state.find_device_key("device-key-1").unwrap();
    assert_eq!(device_key.password_files.len(), 1);
    assert_eq!(device_key.password_files[0].password_file.0, NEW_PF_BYTES);
    assert_eq!(
        device_key.password_files[0].opaque_domain_separator,
        OPAQUE_DOMAIN_SEP
    );
}

#[rstest]
#[case("no_session_state", None)]
#[case(
    "pending_auth",
    Some(SessionState::PendingAuth(PendingAuthData {
        server_login: PendingLoginState::new(vec![1]),
        purpose: None,
    }))
)]
#[case(
    "active_no_ongoing_op",
    Some(SessionState::Active(SessionData {
        session_key: SessionKey::new(vec![1]),
        purpose: None,
        operation: None,
        has_performed_hsm_operation: false,
    }))
)]
fn pin_change_finish_requires_active_changing_pin_state(
    #[case] _label: &str,
    #[case] session_state: Option<SessionState>,
) {
    let op = PinChangeFinishOperation::new(Arc::new(MockPakePort::new()));
    let mut context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::ChangePinFinish),
    );
    context.session_state = session_state;

    let result = op.execute(context);

    assert!(
        matches!(result, Err(ServiceRequestError::InvalidOperation)),
        "expected InvalidOperation for label: {_label}"
    );
}

// -----------------------------------------------------------------------------
// PinChangeStartOperation
// -----------------------------------------------------------------------------

#[test]
fn pin_change_start_without_session_fails() {
    let op = PinChangeStartOperation::new(Arc::new(MockPakePort::new()));
    // base_context has session_id: None by default
    let context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::ChangePinStart),
    );

    let result = op.execute(context);

    assert!(matches!(result, Err(ServiceRequestError::UnknownSession)));
}

#[test]
fn pin_change_start_with_session_succeeds() {
    let mut mock = MockPakePort::new();
    mock.expect_registration_start()
        .once()
        .returning(|_, _| Ok(PakePayloadVector::new(vec![4, 5, 6])));
    let op = PinChangeStartOperation::new(Arc::new(mock));

    let mut context = base_context(
        state_without_password_file(),
        pake_inner_request(OperationId::ChangePinStart),
    );
    context.session_id = Some(SessionId::new());

    let result = op.execute(context);

    assert!(result.is_ok(), "expected Ok");
    assert!(
        matches!(
            result.unwrap().session_transition,
            Some(SessionTransition::BeginChangingPin)
        ),
        "expected BeginChangingPin transition"
    );
}
