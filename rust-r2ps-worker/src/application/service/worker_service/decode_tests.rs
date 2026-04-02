use crate::application::port::outgoing::jose_port::{JoseError, MockJosePort};
use crate::application::service::worker_service::decode::RequestDecoder;
use crate::application::service::worker_service::error::{OuterError, UpstreamError, WorkerError};
use crate::application::service::worker_service::test_utils::{
    make_outer, make_request, make_state,
};
use std::sync::Arc;

#[test]
fn test_decode_outer_error_server_verify_fails() {
    let mut mock_jose = MockJosePort::new();
    mock_jose
        .expect_jws_verify_server()
        .returning(|_| Err(JoseError::VerifyError));
    let decoder = RequestDecoder::new(Arc::new(mock_jose), true);

    let result = decoder.decode_outer(make_request("req"));

    assert_eq!(
        result.err().unwrap(),
        WorkerError::Upstream(UpstreamError::InvalidStateJws)
    );
}

#[test]
fn test_decode_outer_error_peek_kid_returns_none() {
    let state_bytes = serde_json::to_vec(&make_state("device-kid")).unwrap();
    let mut mock_jose = MockJosePort::new();
    mock_jose
        .expect_jws_verify_server()
        .returning(move |_| Ok(state_bytes.clone()));
    mock_jose.expect_peek_kid().returning(|_| Ok(None));
    let decoder = RequestDecoder::new(Arc::new(mock_jose), true);

    let result = decoder.decode_outer(make_request("req"));

    assert_eq!(
        result.err().unwrap(),
        WorkerError::Upstream(UpstreamError::OuterJwsMissingKid)
    );
}

#[test]
fn test_decode_outer_error_kid_not_in_state() {
    let state_bytes = serde_json::to_vec(&make_state("device-kid")).unwrap();
    let mut mock_jose = MockJosePort::new();
    mock_jose
        .expect_jws_verify_server()
        .returning(move |_| Ok(state_bytes.clone()));
    mock_jose
        .expect_peek_kid()
        .returning(|_| Ok(Some("unknown-kid".to_string())));
    let decoder = RequestDecoder::new(Arc::new(mock_jose), true);

    let result = decoder.decode_outer(make_request("req"));

    assert_eq!(
        result.err().unwrap(),
        WorkerError::Upstream(UpstreamError::UnknownDevice)
    );
}

#[test]
fn test_decode_outer_returns_unsupported_context_when_context_is_not_hsm() {
    let state_bytes = serde_json::to_vec(&make_state("device-kid")).unwrap();
    let outer_bytes = serde_json::to_vec(&make_outer("not-hsm", None)).unwrap();
    let mut mock_jose = MockJosePort::new();
    mock_jose
        .expect_jws_verify_server()
        .returning(move |_| Ok(state_bytes.clone()));
    mock_jose
        .expect_peek_kid()
        .returning(|_| Ok(Some("device-kid".to_string())));
    mock_jose
        .expect_jws_verify_device()
        .returning(move |_, _| Ok(outer_bytes.clone()));
    let decoder = RequestDecoder::new(Arc::new(mock_jose), true);

    assert!(matches!(
        decoder.decode_outer(make_request("req")),
        Err(WorkerError::Outer(OuterError::UnsupportedContext))
    ));
}
