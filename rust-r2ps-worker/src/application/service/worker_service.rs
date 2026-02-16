use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::pending_auth_spi_port::PendingAuthSpiPort;
use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};

use crate::application::{WorkerRequestId, WorkerRequestUseCase, WorkerResponseSpiPort};
use crate::define_byte_vector;
use crate::domain::value_objects::r2ps::{OuterRequest, SessionId};
use crate::domain::{
    DefaultCipherSuite, DeviceHsmState, EncryptOption, HsmWorkerRequest, InnerJwe, InnerRequest,
    OperationId, OuterResponse, ServiceRequestError, WorkerRequestError, WorkerResponseJws,
    WorkerServerConfig,
};
use argon2::password_hash::rand_core::OsRng;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use josekit::jwe;
use josekit::jwe::{ECDH_ES, JweHeader};
use josekit::jws::ES256;
use josekit::jws::alg::ecdsa::{EcdsaJwsSigner, EcdsaJwsVerifier};
use josekit::jwt::{self, JwtPayload};
use opaque_ke::ServerSetup;
use opaque_ke::keypair::{KeyPair, PrivateKey, PublicKey};
use p256::NistP256;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use pem::Pem;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info};

define_byte_vector!(DecryptedData);

use super::operations::{OperationContext, OperationDispatcher, OperationResult};

pub struct WorkerService {
    worker_response_spi_port: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    worker_server_config: WorkerServerConfig,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    // Operation dispatcher
    operation_dispatcher: OperationDispatcher,
    jws_signer: EcdsaJwsSigner,
    state_jws_verifier: EcdsaJwsVerifier,
}

struct ResponseContext {
    request_id: String,
    request_type: OperationId,
    session_key: Option<SessionKey>,
    device_kid: String,
}

struct WorkerInput {
    operation_context: OperationContext,
    response_context: ResponseContext,
}

pub struct WorkerPorts {
    pub worker_response: Arc<dyn WorkerResponseSpiPort + Send + Sync>,
    pub session_key: Arc<dyn SessionKeySpiPort + Send + Sync>,
    pub hsm: Arc<dyn HsmSpiPort + Send + Sync>,
    pub pending_auth: Arc<dyn PendingAuthSpiPort + Send + Sync>,
}

pub struct OpaqueConfig {
    pub opaque_server_setup: Option<String>,
    pub opaque_context: String,
    pub opaque_server_identifier: String,
}

impl WorkerService {
    pub fn new(
        server_public_key: Pem,
        server_private_key: Pem,
        ports: WorkerPorts,
        opaque_config: OpaqueConfig,
    ) -> Self {
        let server_setup =
            init_server_setup(&opaque_config.opaque_server_setup, &server_private_key);

        let operation_dispatcher = OperationDispatcher::from_dependencies(
            server_setup,
            ports.session_key.clone(),
            ports.hsm,
            ports.pending_auth,
            opaque_config.opaque_context,
            opaque_config.opaque_server_identifier,
        );

        let (jws_signer, state_jws_verifier) =
            jws_crypto_provider(&server_public_key, &server_private_key)
                .expect("Failed to initialize JWS crypto from server keys");

        Self {
            worker_response_spi_port: ports.worker_response,
            worker_server_config: WorkerServerConfig {
                server_public_key,
                server_private_key,
            },
            operation_dispatcher,
            session_key_spi_port: ports.session_key,
            jws_signer,
            state_jws_verifier,
        }
    }

    /// Returns a reference to the server configuration
    pub fn server_config(&self) -> &WorkerServerConfig {
        &self.worker_server_config
    }

    fn decrypt_inner_request(
        &self,
        inner_jwe: Option<&InnerJwe>,
        session_key: Option<&SessionKey>,
    ) -> Result<InnerRequest, ServiceRequestError> {
        let Some(jwe) = inner_jwe else {
            return Err(ServiceRequestError::JweError);
        };

        let peeked_kid = jwe.peek_kid().map_err(|_| ServiceRequestError::JweError)?;
        debug!("Peeked inner JWE kid: {:?}", peeked_kid);

        // parse peeked_kid into EncryptOption
        let enc_option = match peeked_kid.as_deref() {
            Some("session") => EncryptOption::Session,
            Some("device") => EncryptOption::Device,
            _ => {
                error!("Unknown encryption option in JWE kid: {:?}", peeked_kid);
                return Err(ServiceRequestError::JweError);
            }
        };

        debug!("Decrypting inner request using {:?} encryption", enc_option);

        let inner_request = jwe
            .decrypt(
                enc_option,
                &self.worker_server_config.server_private_key,
                session_key,
            )
            .map_err(|e| {
                error!("Could not decrypt inner request: {:?}", e);
                ServiceRequestError::JweError
            })?;

        if inner_request.request_type.encrypt_option() != enc_option {
            error!(
                "Encryption option for type {:?} mismatch: expected {:?}, decrypted JWE using {:?}",
                inner_request.request_type,
                inner_request.request_type.encrypt_option(),
                enc_option
            );
            return Err(ServiceRequestError::JweError);
        }

        Ok(inner_request)
    }

    fn create_outer_response(
        &self,
        inner_jwe: InnerJwe,
        session_id: Option<&SessionId>,
    ) -> Result<String, ServiceRequestError> {
        let outer_response = OuterResponse {
            version: 1,
            inner_jwe: Some(inner_jwe),
            session_id: session_id.cloned(),
        };

        debug!("Outer response before JWS encoding: {:#?}", outer_response);

        // Create JWT payload from outer_response
        let value = serde_json::to_value(&outer_response).map_err(|e| {
            error!("Failed to serialize outer response: {:?}", e);
            ServiceRequestError::SerializeResponseError
        })?;

        let map = value.as_object().cloned().ok_or_else(|| {
            error!("Failed to convert outer response to JSON object");
            ServiceRequestError::SerializeResponseError
        })?;

        let payload = JwtPayload::from_map(map).map_err(|e| {
            error!("Failed to create JwtPayload: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        // Create JWS header
        let header = josekit::jws::JwsHeader::new();

        let token = jwt::encode_with_signer(&payload, &header, &self.jws_signer).map_err(|e| {
            error!("Failed to encode outer response JWS: {:?}", e);
            ServiceRequestError::JwsError
        })?;

        Ok(token)
    }

    fn decode_outer_request_jws(
        &self,
        outer_request_jws: String,
        client_public_key: &josekit::jwk::Jwk,
    ) -> Result<OuterRequest, ServiceRequestError> {
        // Create verifier from JWK using ES256 algorithm
        let verifier = ES256.verifier_from_jwk(client_public_key).map_err(|e| {
            error!("Failed to create verifier from JWK: {:?}", e);
            ServiceRequestError::InvalidClientPublicKey
        })?;

        // Decode and verify JWT
        let (payload, _header) =
            jwt::decode_with_verifier(&outer_request_jws, &verifier).map_err(|e| {
                error!("JWS verification failed: {:?}", e);
                ServiceRequestError::JwsError
            })?;

        // Deserialize payload to OuterRequest
        let outer_request: OuterRequest =
            serde_json::from_str(&payload.to_string()).map_err(|e| {
                error!("Failed to deserialize outer request: {:?}", e);
                ServiceRequestError::JwsError
            })?;

        debug!("decoded outer request JWS: {:#?}", outer_request);
        Ok(outer_request)
    }

    fn input(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerInput, WorkerRequestError> {
        let HsmWorkerRequest {
            request_id,
            state_jws,
            outer_request_jws,
        } = hsm_worker_request;

        let state = DeviceHsmState::decode_from_jws(&state_jws, &self.state_jws_verifier)
            .map_err(|_| WorkerRequestError::InvalidState)?;

        // Extract client public key kid from JWS header
        let device_kid = peek_jws_kid(&outer_request_jws)
            .map_err(|_| WorkerRequestError::OuterJwsError)?
            .ok_or(WorkerRequestError::OuterJwsError)?;

        debug!("Peeked outer request JWS kid: {}", device_kid);

        // Fetch the corresponding JWK from state using kid
        let client_public_key = state
            .find_device_key(&device_kid)
            .ok_or(WorkerRequestError::OuterJwsError)?
            .public_key
            .clone();

        let outer_request = self
            .decode_outer_request_jws(outer_request_jws, &client_public_key)
            .map_err(|_| WorkerRequestError::OuterJwsError)?;

        info!(
            "Received request id {}, client_id {}",
            request_id, state.client_id
        );

        // TODO: Use JOSE 'aud' (audience) claim in the validation done inside decode_service_request_jws() instead
        if outer_request.context != "hsm" {
            return Err(WorkerRequestError::UnsupportedContext);
        }

        let session_id = outer_request.session_id.clone();
        let session_key = session_id
            .as_ref()
            .and_then(|id| self.session_key_spi_port.get(id));

        let inner_request = self
            .decrypt_inner_request(outer_request.inner_jwe.as_ref(), session_key.as_ref())
            .map_err(WorkerRequestError::ServiceError)?;

        debug!("Inner request: {:#?}", inner_request);

        let request_type = inner_request.request_type;

        info!(
            "Processing request id {} of type {:?}",
            request_id, request_type
        );

        let operation_context = OperationContext {
            request_id: request_id.clone(),
            state,
            outer_request: outer_request.clone(),
            inner_request,
            session_id: session_id.clone(),
            device_kid: device_kid.clone(),
        };

        let response_context = ResponseContext {
            request_id,
            request_type,
            session_key,
            device_kid,
        };

        Ok(WorkerInput {
            operation_context,
            response_context,
        })
    }

    fn output(
        &self,
        operation_result: OperationResult,
        context: ResponseContext,
    ) -> Result<WorkerResponseJws, WorkerRequestError> {
        debug!("Operation result: {:#?}", operation_result.data);

        let encoded_result = operation_result
            .data
            .serialize()
            .map_err(|_| WorkerRequestError::EncryptionError)?;

        let ttl = match operation_result.session_id.as_ref() {
            Some(id) => self.session_key_spi_port.get_remaining_ttl(id),
            None => None,
        };

        // Create InnerResponse with the serialized data
        let serialized_data = String::from_utf8(encoded_result.clone())
            .map_err(|_| WorkerRequestError::EncryptionError)?;
        let inner_response = operation_result.to_inner_response(serialized_data, ttl);

        debug!("Inner response: {:#?}", inner_response);

        // Serialize InnerResponse to JSON
        let inner_response_json =
            serde_json::to_vec(&inner_response).map_err(|_| WorkerRequestError::EncryptionError)?;

        let enc_option = context.request_type.encrypt_option();
        debug!(
            "Inner response to {:?} will be encrypted with {:?} encryption",
            context.request_type, enc_option
        );

        // Encrypt the serialized InnerResponse into InnerJwe
        let inner_jwe = match enc_option {
            EncryptOption::Session => {
                let session_key = context
                    .session_key
                    .clone()
                    .ok_or(WorkerRequestError::UnknownSession)?;
                InnerJwe::encrypt(&inner_response_json, &session_key)
                    .map_err(|_| WorkerRequestError::EncryptionError)?
            }
            EncryptOption::Device => {
                let client_public_key = operation_result
                    .state
                    .find_device_key(&context.device_kid)
                    .ok_or(WorkerRequestError::EncryptionError)?
                    .public_key
                    .clone();
                encrypt_with_ec_jwk(&inner_response_json, &client_public_key)
                    .map_err(|_| WorkerRequestError::EncryptionError)?
            }
        };

        let jws = self
            .create_outer_response(inner_jwe, operation_result.session_id.as_ref())
            .map_err(|_| WorkerRequestError::OuterJwsError)?;

        let new_state_jws = operation_result
            .state
            .encode_to_jws(&self.jws_signer)
            .map_err(|_| WorkerRequestError::OuterJwsError)?;

        Ok(WorkerResponseJws {
            request_id: context.request_id,
            device_id: operation_result.state.client_id.clone(),
            http_status: 200,
            state_jws: new_state_jws,
            service_response_jws: jws,
        })
    }
}

fn init_server_setup(
    opaque_server_setup: &Option<String>,
    server_private_key: &Pem,
) -> ServerSetup<DefaultCipherSuite> {
    match load_server_setup(opaque_server_setup) {
        Ok(setup) => setup,
        Err(_e) => {
            let setup = create_server_setup(server_private_key)
                .expect("Failed to create opaque server setup");
            info!(
                "OPAQUE_SERVER_SETUP={}",
                BASE64_STANDARD.encode(setup.serialize())
            );
            setup
        }
    }
}

fn jws_crypto_provider(
    server_public_key: &Pem,
    server_private_key: &Pem,
) -> Result<(EcdsaJwsSigner, EcdsaJwsVerifier), ServiceRequestError> {
    // Create signer from PEM
    let pem_string = pem::encode(server_private_key);
    let jws_signer = ES256.signer_from_pem(pem_string.as_bytes()).map_err(|e| {
        error!(
            "Failed to create signer from server private key PEM: {:?}",
            e
        );
        ServiceRequestError::JwsError
    })?;

    // Create verifier from PEM
    let public_key_pem = pem::encode(server_public_key);
    let state_jws_verifier = ES256.verifier_from_pem(&public_key_pem).map_err(|e| {
        error!(
            "Failed to create verifier from server public key PEM: {:?}",
            e
        );
        ServiceRequestError::JwsError
    })?;
    Ok((jws_signer, state_jws_verifier))
}

impl WorkerRequestUseCase for WorkerService {
    fn execute(
        &self,
        hsm_worker_request: HsmWorkerRequest,
    ) -> Result<WorkerRequestId, WorkerRequestError> {
        let start = Instant::now();
        let WorkerInput {
            operation_context,
            response_context,
        } = self.input(hsm_worker_request)?;

        let request_id = response_context.request_id.clone();
        let request_type = response_context.request_type;

        let operation_result = self
            .operation_dispatcher
            .dispatch(operation_context)
            .map_err(WorkerRequestError::ServiceError)?;

        let worker_response_jws = self.output(operation_result, response_context)?;

        let processing_elapsed = start.elapsed();
        debug!(
            "Request {:?} total processing time: {} ms",
            request_type,
            processing_elapsed.as_millis()
        );

        self.worker_response_spi_port
            .send(worker_response_jws)
            .map_err(|_| WorkerRequestError::ConnectionError)?;

        let finished_elapsed = start.elapsed();

        info!(
            "Responding to request id {} ({:?}, took {}/{} ms)",
            request_id,
            request_type,
            processing_elapsed.as_millis(),
            finished_elapsed.as_millis()
        );

        Ok(request_id)
    }
}

fn create_server_setup(
    server_private_key_pem: &Pem,
) -> Result<ServerSetup<DefaultCipherSuite>, String> {
    let secret_key = p256::SecretKey::from_pkcs8_pem(&pem::encode(server_private_key_pem))
        .map_err(|e| format!("Failed to parse P-256 private key: {:?}", e))?;

    let keypair = KeyPair::new(
        PrivateKey::<NistP256>::deserialize(&secret_key.to_bytes())
            .map_err(|e| format!("Failed to deserialize private key: {:?}", e))?,
        PublicKey::<NistP256>::deserialize(
            secret_key
                .public_key()
                .as_affine()
                .to_encoded_point(true)
                .as_bytes(),
        )
        .map_err(|e| format!("Failed to deserialize public key: {:?}", e))?,
    );

    Ok(ServerSetup::<DefaultCipherSuite>::new_with_key_pair(
        &mut OsRng, keypair,
    ))
}

fn load_server_setup(
    server_setup: &Option<String>,
) -> Result<ServerSetup<DefaultCipherSuite>, String> {
    match server_setup {
        Some(server_setup_hex) => {
            let bytes = BASE64_STANDARD
                .decode(server_setup_hex.as_bytes())
                .map_err(|e| format!("Failed to decode server setup hex: {}", e))?;

            // Deserialize from bytes
            ServerSetup::deserialize(&bytes)
                .map_err(|e| format!("Failed to deserialize server setup: {}", e))
        }
        None => Err("Invalid server setup".to_string()),
    }
}

fn encrypt_with_ec_jwk(
    payload: &[u8],
    client_public_key: &josekit::jwk::Jwk,
) -> Result<InnerJwe, ServiceRequestError> {
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");
    header.set_key_id("device");

    match ECDH_ES.encrypter_from_jwk(client_public_key) {
        Ok(encrypter) => match jwe::serialize_compact(payload, &header, &encrypter) {
            Ok(payload_bytes) => Ok(InnerJwe::new(payload_bytes)),
            Err(e) => {
                error!("JWE encryption failed: {:?}", e);
                Err(ServiceRequestError::JweError)
            }
        },
        Err(e) => {
            error!("Failed to create encrypter from JWK: {:?}", e);
            Err(ServiceRequestError::JweError)
        }
    }
}

fn peek_jws_kid(jws: &str) -> Result<Option<String>, ServiceRequestError> {
    use base64::prelude::BASE64_URL_SAFE_NO_PAD;

    // Split JWS compact serialization (header.payload.signature)
    let parts: Vec<&str> = jws.split('.').collect();
    if parts.len() < 3 {
        return Err(ServiceRequestError::JwsError);
    }

    // Decode the header (first part)
    let header_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| ServiceRequestError::JwsError)?;

    let header: serde_json::Value =
        serde_json::from_slice(&header_bytes).map_err(|_| ServiceRequestError::JwsError)?;

    Ok(header
        .get("kid")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}
