use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::pending_auth_spi_port::PendingAuthSpiPort;
use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use crate::application::{
    R2psRequestId, R2psRequestUseCase, R2psResponseSpiPort, load_pem_from_bas64_env,
};
use crate::define_byte_vector;
use crate::domain::value_objects::r2ps::{OuterRequest, SessionId};
use crate::domain::{
    DefaultCipherSuite, DeviceHsmState, EncryptOption, InnerJwe, InnerRequest, OuterResponse,
    R2psRequestError, R2psRequestJws, R2psResponseJws, R2psServerConfig, ServiceRequestError,
};
use crate::infrastructure::ec_jwk_to_pem;
use argon2::password_hash::rand_core::OsRng;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use josekit::jwe;
use josekit::jwe::{ECDH_ES, JweHeader};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use opaque_ke::ServerSetup;
use opaque_ke::keypair::{KeyPair, PrivateKey, PublicKey};
use p256::NistP256;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use pem::Pem;
use std::env;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info};

define_byte_vector!(DecryptedData);

use super::operations::{OperationContext, OperationDispatcher};

pub struct R2psService {
    r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
    r2ps_server_config: R2psServerConfig,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    // Operation dispatcher
    operation_dispatcher: OperationDispatcher,
}

impl R2psService {
    pub fn new(
        r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
        hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
        pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
    ) -> Self {
        let server_public_key =
            load_pem_from_bas64_env("SERVER_PUBLIC_KEY").expect("Failed to load SERVER_PUBLIC_KEY");
        let server_private_key = load_pem_from_bas64_env("SERVER_PRIVATE_KEY")
            .expect("Failed to load SERVER_PRIVATE_KEY");

        let server_setup = match load_server_setup("SERVER_SETUP") {
            Ok(setup) => setup,
            Err(_e) => {
                let setup = create_server_setup(&server_private_key)
                    .expect("Failed to create opaque server setup");
                info!("SERVER_SETUP={}", BASE64_STANDARD.encode(setup.serialize()));
                setup
            }
        };

        let operation_dispatcher = OperationDispatcher::from_dependencies(
            server_setup,
            session_key_spi_port.clone(),
            hsm_spi_port,
            pending_auth_spi_port,
        );

        Self {
            r2ps_response_spi_port,
            r2ps_server_config: R2psServerConfig {
                server_public_key,
                server_private_key,
            },
            operation_dispatcher,
            session_key_spi_port,
        }
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
                &self.r2ps_server_config.server_private_key,
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
}

impl R2psRequestUseCase for R2psService {
    fn execute(&self, r2ps_request_jws: R2psRequestJws) -> Result<R2psRequestId, R2psRequestError> {
        let start = Instant::now();

        let state = decode_state_jws(
            r2ps_request_jws.state_jws,
            &self.r2ps_server_config.server_public_key,
        )
        .map_err(|_| R2psRequestError::InvalidState)?;

        let client_public_key = ec_jwk_to_pem(&state.client_public_key).map_err(|_| {
            R2psRequestError::ServiceError(ServiceRequestError::InvalidClientPublicKey)
        })?;
        let outer_request =
            decode_outer_request_jws(r2ps_request_jws.outer_request_jws, &client_public_key)
                .map_err(|_| R2psRequestError::OuterJwsError)?;

        info!(
            "Received request id {}, wallet_id {}",
            r2ps_request_jws.request_id, r2ps_request_jws.wallet_id
        );

        // TODO: Use JOSE 'aud' (audience) claim in the validation done inside decode_service_request_jws() instead
        if outer_request.context != "hsm" {
            return Err(R2psRequestError::UnsupportedContext);
        }

        let session_id = outer_request.session_id.as_ref();
        let session_key = session_id
            .as_ref()
            .and_then(|id| self.session_key_spi_port.get(id));

        let inner_request = self
            .decrypt_inner_request(outer_request.inner_jwe.as_ref(), session_key.as_ref())
            .map_err(R2psRequestError::ServiceError)?;

        debug!("Inner request: {:#?}", inner_request);

        let request_type = inner_request.request_type;

        info!(
            "Processing request id {} of type {:?}",
            r2ps_request_jws.request_id, request_type
        );

        let context = OperationContext {
            request_id: r2ps_request_jws.request_id.clone(),
            wallet_id: r2ps_request_jws.wallet_id.clone(),
            device_id: r2ps_request_jws.device_id.clone(),
            state: state,
            outer_request: outer_request.clone(),
            inner_request: inner_request,
            session_id: session_id.cloned(),
        };

        let operation_result = self
            .operation_dispatcher
            .dispatch(context)
            .map_err(R2psRequestError::ServiceError)?;

        debug!("Operation result: {:#?}", operation_result.data);

        let encoded_result = operation_result
            .data
            .serialize()
            .map_err(|_| R2psRequestError::EncryptionError)?;

        let ttl = match session_id.as_ref() {
            Some(id) => self.session_key_spi_port.get_remaining_ttl(id),
            None => None,
        };

        // Create InnerResponse with the serialized data
        let serialized_data = String::from_utf8(encoded_result.clone())
            .map_err(|_| R2psRequestError::EncryptionError)?;
        let inner_response = operation_result.to_inner_response(serialized_data, ttl);

        debug!("Inner response: {:#?}", inner_response);

        // Serialize InnerResponse to JSON
        let inner_response_json =
            serde_json::to_vec(&inner_response).map_err(|_| R2psRequestError::EncryptionError)?;

        let enc_option = request_type.encrypt_option().clone();
        debug!(
            "Inner response to {:?} will be encrypted with {:?} encryption",
            request_type, enc_option
        );

        // Encrypt the serialized InnerResponse into InnerJwe
        let inner_jwe = match enc_option {
            EncryptOption::Session => {
                let session_key = session_key
                    .clone()
                    .ok_or(R2psRequestError::UnknownSession)?;
                InnerJwe::encrypt(&inner_response_json, &session_key)
                    .map_err(|_| R2psRequestError::EncryptionError)?
            }
            EncryptOption::Device => {
                let public_key = ec_jwk_to_pem(&operation_result.state.client_public_key)
                    .map_err(|_| R2psRequestError::EncryptionError)?;
                encrypt_with_ec_pem(&inner_response_json, &public_key)
                    .map_err(|_| R2psRequestError::EncryptionError)?
            }
        };

        let jws = create_outer_response(inner_jwe, operation_result.session_id.as_ref())
            .map_err(|_| R2psRequestError::OuterJwsError)?;

        let new_state_jws = encode_state_jws(&operation_result.state)
            .map_err(|_| R2psRequestError::OuterJwsError)?;

        let r2ps_response_jws = R2psResponseJws {
            request_id: r2ps_request_jws.request_id.clone(),
            wallet_id: r2ps_request_jws.wallet_id.clone(),
            device_id: r2ps_request_jws.device_id.clone(),
            http_status: 200,
            state_jws: new_state_jws,
            service_response_jws: jws,
        };

        let processing_elapsed = start.elapsed();
        debug!(
            "Request {:?} total processing time: {} ms",
            request_type,
            processing_elapsed.as_millis()
        );

        let request_id = self
            .r2ps_response_spi_port
            .send(r2ps_response_jws.clone())
            .map(|_| r2ps_response_jws.request_id.clone())
            .map_err(|_| R2psRequestError::ConnectionError)?;

        let finished_elapsed = start.elapsed();

        info!(
            "Responding to request id {} ({:?}, took {}/{} ms)",
            r2ps_request_jws.request_id,
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

fn load_server_setup(env_var_name: &str) -> Result<ServerSetup<DefaultCipherSuite>, String> {
    match env::var(env_var_name) {
        Ok(server_setup_hex) => {
            let bytes = BASE64_STANDARD
                .decode(server_setup_hex.as_bytes())
                .map_err(|e| format!("Failed to decode server setup hex: {}", e))?;

            // Deserialize from bytes
            ServerSetup::deserialize(&bytes)
                .map_err(|e| format!("Failed to deserialize server setup: {}", e))
        }
        Err(_e) => {
            error!("Missing server setup config in {}", env_var_name);
            Err("Invalid server setup".to_string())
        }
    }
}

fn encrypt_with_ec_pem(
    payload: &[u8],
    client_public_key: &Pem,
) -> Result<InnerJwe, ServiceRequestError> {
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");
    header.set_key_id("device");

    let pem_string = pem::encode(client_public_key);
    match ECDH_ES.encrypter_from_pem(&pem_string) {
        Ok(encrypter) => match jwe::serialize_compact(payload, &header, &encrypter) {
            Ok(payload_bytes) => Ok(InnerJwe::new(payload_bytes)),
            Err(e) => {
                error!("********1 {:?}", e);
                Err(ServiceRequestError::Unknown)
            }
        },
        Err(e) => {
            error!("********2 {:?}", e);
            Err(ServiceRequestError::Unknown)
        }
    }
}

fn create_outer_response(
    inner_jwe: InnerJwe,
    session_id: Option<&SessionId>,
) -> Result<String, ServiceRequestError> {
    let outer_response = OuterResponse {
        version: 1,
        inner_jwe: Some(inner_jwe),
        session_id: session_id.cloned(),
    };

    debug!("Outer response before JWS encoding: {:#?}", outer_response);

    let mut header = Header::new(Algorithm::ES256);
    header.typ = Some("JOSE".to_string());

    let private_key_pem = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg/NIIdRGO+qU2bjxT
tnZuC45gAg6wZ0UGe9nCeM7wc0yhRANCAASnNDG5ct6I/LOK0wpBtRJU4PcDFv6X
0upWOzkadhqcDWTgCYxROhakhPDldczjw0+FuAyGgzQVSng5DbrP+8JB
-----END PRIVATE KEY-----"#;

    let encoding_key = EncodingKey::from_ec_pem(private_key_pem.as_bytes()).unwrap();

    let token = encode(&header, &outer_response, &encoding_key).unwrap();

    Ok(token)
}

fn encode_state_jws(state: &DeviceHsmState) -> Result<String, ServiceRequestError> {
    let mut header = Header::new(Algorithm::ES256);
    header.typ = Some("JOSE".to_string());

    let private_key_pem = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg/NIIdRGO+qU2bjxT
tnZuC45gAg6wZ0UGe9nCeM7wc0yhRANCAASnNDG5ct6I/LOK0wpBtRJU4PcDFv6X
0upWOzkadhqcDWTgCYxROhakhPDldczjw0+FuAyGgzQVSng5DbrP+8JB
-----END PRIVATE KEY-----"#;

    let encoding_key = EncodingKey::from_ec_pem(private_key_pem.as_bytes()).unwrap();

    let token = encode(&header, &state, &encoding_key).unwrap();

    Ok(token)
}

fn decode_state_jws(
    state_jws: String,
    public_key: &Pem,
) -> Result<DeviceHsmState, ServiceRequestError> {
    let pem_string = pem::encode(public_key);

    // TODO: Compute DecodingKey once on startup and reuse it
    match DecodingKey::from_ec_pem(pem_string.as_bytes()) {
        Ok(decoding_key) => {
            let mut validation = Validation::new(Algorithm::ES256);
            validation.validate_exp = false;
            validation.required_spec_claims.clear();
            validation.insecure_disable_signature_validation();
            match decode::<DeviceHsmState>(&state_jws, &decoding_key, &validation) {
                Ok(service_request_claims) => {
                    debug!("decoded state JWS: {:#?}", service_request_claims);
                    Ok(service_request_claims.claims)
                }
                Err(error) => {
                    error!("Error decoding state JWS: {:#?}", error);
                    Err(ServiceRequestError::JwsError)
                }
            }
        }
        Err(error) => {
            error!("invalid client public key: {:?}", error);
            Err(ServiceRequestError::InvalidClientPublicKey)
        }
    }
}
fn decode_outer_request_jws(
    outer_request_jws: String,
    client_public_key: &Pem,
) -> Result<OuterRequest, ServiceRequestError> {
    let pem_string = pem::encode(client_public_key);

    // TODO: This key is converted from the JWK in the state to PEM and then from PEM into a DecodingKey.
    // If we update the state to use the right JWK type (it currently does not), we can create the
    // DecodingKey directly from the JWK and avoid one conversion.
    match DecodingKey::from_ec_pem(pem_string.as_bytes()) {
        Ok(decoding_key) => {
            let mut validation = Validation::new(Algorithm::ES256);
            validation.validate_exp = false;
            validation.required_spec_claims.clear();
            match decode::<OuterRequest>(&outer_request_jws, &decoding_key, &validation) {
                Ok(service_request_claims) => {
                    debug!("decoded outer request JWS: {:#?}", service_request_claims);
                    Ok(service_request_claims.claims)
                }
                Err(error) => {
                    error!("Error decoding outer request JWS: {:#?}", error);
                    Err(ServiceRequestError::JwsError)
                }
            }
        }
        Err(error) => {
            error!("invalid client public key: {:?}", error);
            Err(ServiceRequestError::InvalidClientPublicKey)
        }
    }
}
