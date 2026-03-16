use crate::domain::device_management::value_objects::EcPublicKey;

/// Represents a state initialization request to the HSM worker.
///
/// Used when registering a new device — sends the public key to the HSM
/// worker to generate the initial device state. State-init is now sent
/// as a regular command on `r2ps-requests` with `OperationId::StateInit`
/// context — no separate topic.
#[derive(Debug, Clone)]
pub struct StateInitRequest {
    request_id: String,
    client_id: String,
    public_key: EcPublicKey,
}

impl StateInitRequest {
    pub fn new(request_id: String, client_id: String, public_key: EcPublicKey) -> Self {
        Self {
            request_id,
            client_id,
            public_key,
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn public_key(&self) -> &EcPublicKey {
        &self.public_key
    }
}

/// Represents a state initialization response from the HSM worker.
///
/// Now arrives on `r2ps-responses` as a regular worker response, routed
/// by correlation_id. The `dev_authorization_code` is inside the encrypted
/// `InnerResponse` payload (the `service_response_jws`).
#[derive(Debug, Clone)]
pub struct StateInitResponse {
    correlation_id: String,
    http_status: u16,
    service_response_jws: String,
}

impl StateInitResponse {
    pub fn new(correlation_id: String, http_status: u16, service_response_jws: String) -> Self {
        Self {
            correlation_id,
            http_status,
            service_response_jws,
        }
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub fn http_status(&self) -> u16 {
        self.http_status
    }

    pub fn service_response_jws(&self) -> &str {
        &self.service_response_jws
    }

    pub fn is_success(&self) -> bool {
        self.http_status == 200
    }
}
