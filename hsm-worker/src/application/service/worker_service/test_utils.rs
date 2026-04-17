// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::hsm_spi_port::MockHsmSpiPort;
use crate::application::jose_port::{JoseError, JosePort, JweDecryptionKey, JweEncryptionKey};
use crate::application::pake_port::MockPakePort;
use crate::application::port::outgoing::session_state_spi_port::{
    SessionData, SessionKey, SessionState, SessionStateError, SessionStateSpiPort,
    SessionTransition,
};
use crate::application::{WorkerPorts, WorkerResponseError, WorkerResponseSpiPort};
use crate::domain::{
    DeviceHsmState, DeviceKeyEntry, EcPublicJwk, HsmWorkerRequest, HsmWorkerResponse, OuterRequest,
    PasswordFile, PasswordFileEntry, SessionId, TypedJwe, TypedJws,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// --- Jose Mocks ---

pub struct MockJoseDeterministic {
    pub state_json: Vec<u8>,
    pub outer_json: Vec<u8>,
    pub inner_json: Vec<u8>,
    pub inner_kid: String,
    pub fail_sign: bool,
    pub captured_inner_encrypt_payload: Mutex<Vec<Vec<u8>>>,
}

impl MockJoseDeterministic {
    pub fn new(
        state: &DeviceHsmState,
        outer: &OuterRequest,
        inner: &[u8],
        inner_kid: &str,
        fail_sign: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            state_json: serde_json::to_vec(state).unwrap(),
            outer_json: serde_json::to_vec(outer).unwrap(),
            inner_json: inner.to_vec(),
            inner_kid: inner_kid.to_string(),
            fail_sign,
            captured_inner_encrypt_payload: Mutex::new(vec![]),
        })
    }
}

impl JosePort for MockJoseDeterministic {
    fn jws_sign(&self, _payload_json: &[u8]) -> Result<String, JoseError> {
        if self.fail_sign {
            Err(JoseError::SignError)
        } else {
            Ok("signed.outer.response".to_string())
        }
    }
    fn jws_verify_server(&self, _jws: &str) -> Result<Vec<u8>, JoseError> {
        Ok(self.state_json.clone())
    }
    fn jws_verify_device(&self, _jws: &str, _key: &EcPublicJwk) -> Result<Vec<u8>, JoseError> {
        Ok(self.outer_json.clone())
    }
    fn jwe_encrypt<'a>(
        &self,
        payload: &[u8],
        _key: JweEncryptionKey<'a>,
    ) -> Result<String, JoseError> {
        self.captured_inner_encrypt_payload
            .lock()
            .unwrap()
            .push(payload.to_vec());
        Ok("encrypted.inner.response".to_string())
    }
    fn jwe_decrypt<'a>(
        &self,
        _jwe: &str,
        _key: JweDecryptionKey<'a>,
    ) -> Result<Vec<u8>, JoseError> {
        Ok(self.inner_json.clone())
    }
    fn peek_kid(&self, compact: &str) -> Option<String> {
        match compact {
            "outer.jws" => Some("device-kid".to_string()),
            "inner.jwe" => Some(self.inner_kid.clone()),
            _ => None,
        }
    }
    fn jws_public_key(&self) -> &EcPublicJwk {
        unimplemented!()
    }
    fn jws_kid(&self) -> &str {
        "mock-kid"
    }
}

// --- SPI Mocks ---

pub struct MockSessionStateSpi;
impl SessionStateSpiPort for MockSessionStateSpi {
    fn get(&self, _id: &SessionId) -> Option<SessionState> {
        None
    }
    fn apply_transition(
        &self,
        _session_id: Option<&SessionId>,
        _transition: Option<&SessionTransition>,
    ) -> Result<(), SessionStateError> {
        Ok(())
    }
    fn get_remaining_ttl(&self, _session_id: Option<&SessionId>) -> Option<Duration> {
        Some(Duration::from_secs(30))
    }
}

pub struct MockWorkerResponseSpi {
    pub responses: Mutex<Vec<HsmWorkerResponse>>,
}
impl MockWorkerResponseSpi {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(vec![]),
        })
    }
}
impl WorkerResponseSpiPort for MockWorkerResponseSpi {
    fn send(
        &self,
        worker_response: HsmWorkerResponse,
        _response_topic: &str,
    ) -> Result<(), WorkerResponseError> {
        self.responses.lock().unwrap().push(worker_response);
        Ok(())
    }
}

pub struct FailingWorkerResponseSpi;
impl WorkerResponseSpiPort for FailingWorkerResponseSpi {
    fn send(
        &self,
        _worker_response: HsmWorkerResponse,
        _response_topic: &str,
    ) -> Result<(), WorkerResponseError> {
        Err(WorkerResponseError::ConnectionError)
    }
}

pub struct MockSessionStateTransitionErrorSpi {
    pub key: SessionKey,
}
impl SessionStateSpiPort for MockSessionStateTransitionErrorSpi {
    fn get(&self, _id: &SessionId) -> Option<SessionState> {
        Some(SessionState::Active(SessionData {
            session_key: self.key.clone(),
            purpose: None,
            operation: None,
            has_performed_hsm_operation: false,
        }))
    }
    fn apply_transition(
        &self,
        _session_id: Option<&SessionId>,
        _transition: Option<&SessionTransition>,
    ) -> Result<(), SessionStateError> {
        Err(SessionStateError::Unknown)
    }
    fn get_remaining_ttl(&self, _session_id: Option<&SessionId>) -> Option<Duration> {
        Some(Duration::from_secs(30))
    }
}

// --- Domain Builders ---

pub fn make_state(device_kid: &str) -> DeviceHsmState {
    make_state_impl(device_kid, vec![])
}

pub fn make_state_with_password_file(device_kid: &str) -> DeviceHsmState {
    make_state_impl(
        device_kid,
        vec![PasswordFileEntry {
            password_file: PasswordFile(vec![1, 2, 3]),
            opaque_domain_separator: "rk-202501_opaque-202501".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }],
    )
}

fn make_state_impl(device_kid: &str, password_files: Vec<PasswordFileEntry>) -> DeviceHsmState {
    DeviceHsmState {
        version: 1,
        device_keys: vec![DeviceKeyEntry {
            public_key: EcPublicJwk {
                kty: "EC".to_string(),
                crv: "P-256".to_string(),
                x: "x".to_string(),
                y: "y".to_string(),
                kid: device_kid.to_string(),
            },
            password_files,
            dev_authorization_code: None,
        }],
        hsm_keys: vec![],
    }
}

pub fn make_request(request_id: &str) -> HsmWorkerRequest {
    HsmWorkerRequest {
        request_id: request_id.to_string(),
        state_jws: TypedJws::new("state.jws".to_string()),
        outer_request_jws: TypedJws::new("outer.jws".to_string()),
        response_topic: "test-response-topic".to_string(),
    }
}

pub fn make_outer(context: &str, session_id: Option<SessionId>) -> OuterRequest {
    OuterRequest {
        version: 1,
        session_id,
        context: context.to_string(),
        inner_jwe: Some(TypedJwe::new("inner.jwe".to_string())),
        server_kid: None,
        nonce: "some_nonce".to_string(),
    }
}

pub fn make_ports(
    jose: Arc<dyn JosePort>,
    worker_response: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
) -> WorkerPorts {
    WorkerPorts {
        jose,
        session_state: Arc::new(MockSessionStateSpi),
        hsm: Arc::new(MockHsmSpiPort::new()),
        worker_response,
        pake: Arc::new(MockPakePort::new()),
    }
}
