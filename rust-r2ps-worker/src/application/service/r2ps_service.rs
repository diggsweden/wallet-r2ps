use crate::application::helpers::ByteVector;
use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::pending_auth_spi_port::{LoginSession, PendingAuthSpiPort};
use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use crate::application::{
    R2psRequestId, R2psRequestUseCase, R2psResponseSpiPort, load_pem_from_bas64_env,
};
use crate::define_byte_vector;
use crate::domain::value_objects::r2ps::{
    Claims, PakeRequestPayload, PakeResponsePayload, ServiceRequest,
};
use crate::domain::{
    CreateKeyServiceData, CreateKeyServiceDataResponse, DefaultCipherSuite, DeleteKeyServiceData,
    DeviceHsmState, EncryptOption, KeyInfo, ListKeysResponse, PakeState, R2psRequest,
    R2psRequestError, R2psRequestJws, R2psResponse, R2psResponseJws, R2psServerConfig,
    ServiceRequestError, ServiceResponse, ServiceTypeId, SignRequest,
};
use crate::infrastructure::ec_jwk_to_pem;
use argon2::password_hash::rand_core::OsRng;
use base64::Engine;
use base64::engine::general_purpose;
use base64::engine::general_purpose::STANDARD;
use base64::prelude::BASE64_STANDARD;
use chrono::Utc;
use josekit::jwe;
use josekit::jwe::alg::direct::DirectJweAlgorithm;
use josekit::jwe::{ECDH_ES, JweHeader};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use opaque_ke::keypair::{KeyPair, PrivateKey, PublicKey};
use opaque_ke::{
    CredentialFinalization, CredentialRequest, Identifiers, RegistrationRequest,
    RegistrationUpload, ServerLogin, ServerLoginParameters, ServerRegistration, ServerSetup,
};
use p256::NistP256;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use pem::Pem;
use rdkafka::message::ToBytes;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

define_byte_vector!(DecryptedData);

#[derive(Clone)]
pub struct R2psService {
    r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
    hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
    opaque_server_setup: ServerSetup<DefaultCipherSuite>,
    r2ps_server_config: R2psServerConfig,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
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
        let server_setup =
            create_server_setup(&server_private_key).expect("Failed to create opaque server setup");

        Self {
            r2ps_response_spi_port,
            session_key_spi_port,
            hsm_spi_port,
            opaque_server_setup: server_setup,
            r2ps_server_config: R2psServerConfig {
                server_public_key,
                server_private_key,
            },
            pending_auth_spi_port,
        }
    }

    pub fn encrypt_with_aes(
        &self,
        payload: &[u8],
        session_key: &SessionKey,
    ) -> Result<String, ServiceRequestError> {
        let mut header = JweHeader::new();
        header.set_algorithm("dir");
        header.set_content_encryption("A256GCM");

        let encrypter = DirectJweAlgorithm::Dir
            .encrypter_from_bytes(session_key.to_bytes())
            .map_err(|e| {
                error!("Failed to create encrypter: {:?}", e);
                ServiceRequestError::Unknown
            })?;

        jwe::serialize_compact(payload, &header, &encrypter).map_err(|e| {
            error!("Failed to encrypt: {:?}", e);
            ServiceRequestError::Unknown
        })
    }
    pub fn decrypt_jwe(
        &self,
        encrypted_payload: &str,
        session_key: &SessionKey,
    ) -> Result<DecryptedData, Box<dyn std::error::Error>> {
        match BASE64_STANDARD.decode(encrypted_payload) {
            Ok(data) => {
                // Cast to ByteVector for better debug logging. Remove casting if/when logging is removed.
                // TODO: Only log the bytes if it can't be decoded to UTF-8 (in which case it will be logged as UTF-8)
                let vec = ByteVector::new(data);
                info!("decoded service_data hex: {:02X?}", &vec);
                match String::from_utf8(vec.to_vec()) {
                    Ok(decoded_string) => {
                        info!("decoded service_data utf8: {}", decoded_string);
                        debug!("decrypt with session key {:02X?}", session_key);
                        let decrypter =
                            DirectJweAlgorithm::Dir.decrypter_from_bytes(session_key.to_bytes())?;
                        let (payload, _header) =
                            josekit::jwe::deserialize_compact(&decoded_string, &decrypter)?;
                        Ok(DecryptedData::new(payload))
                    }
                    Err(_) => Err(Box::new(std::io::Error::other("Failed to decode UTF-8"))),
                }
            }
            Err(_) => Err(Box::new(std::io::Error::other("Failed to decode base64"))),
        }
    }

    pub(crate) fn authenticate(
        &self,
        r2ps_request: &R2psRequest,
        decrypted_service_data: Option<DecryptedData>,
    ) -> Result<R2psResponse, ServiceRequestError> {
        let start = Instant::now();

        let data =
            decrypted_service_data.ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let pake_payload = PakeRequestPayload::deserialize(data.to_vec()).map_err(|e| {
            warn!("error decoding pake request: {:?}", e);
            ServiceRequestError::InvalidPakeRequestPayload
        })?;

        info!(
            "deserialized pake payload authenticate request data: {}",
            pake_payload.request_data
        );

        // Handle Java jackson double base64 encoding....TODO remove later

        let decoded_request_data: Vec<u8> = general_purpose::STANDARD
            .decode(pake_payload.request_data)
            .map_err(|e| {
                warn!("error base64 decoding pake authenticate request: {:?}", e);
                ServiceRequestError::InvalidPakeRequestPayload
            })?;

        match pake_payload.state {
            PakeState::Evaluate => {
                let password_file_serialized = r2ps_request
                    .state
                    .password_file
                    .ok_or(ServiceRequestError::UnknownClient)?;

                let password_file = ServerRegistration::<DefaultCipherSuite>::deserialize(
                    &password_file_serialized,
                )
                .map_err(|e| {
                    warn!("error decoding pake request: {:?}", e);
                    ServiceRequestError::InvalidSerializedPasswordFile
                })?;

                let credential_request = CredentialRequest::deserialize(&decoded_request_data)
                    .map_err(|e| {
                        warn!("error decoding pake request: {:?}", e);
                        ServiceRequestError::InvalidAuthenticateRequest
                    })?;

                let mut server_rng = OsRng;
                let context = "RPS-Ops".as_bytes();
                let client = r2ps_request.device_id.as_bytes();
                let server = "https://cloud-wallet.digg.se/rhsm".as_bytes();

                info!(
                    "OPAQUE context: '{}' hex: {}",
                    String::from_utf8_lossy(context),
                    hex::encode(context)
                );
                info!(
                    "OPAQUE client: '{}' hex: {}",
                    String::from_utf8_lossy(client),
                    hex::encode(client)
                );
                info!(
                    "OPAQUE server: '{}' hex: {}",
                    String::from_utf8_lossy(server),
                    hex::encode(server)
                );

                let server_login_parameters = ServerLoginParameters {
                    context: Some(context),
                    identifiers: Identifiers {
                        client: Some(client),
                        server: Some(server),
                    },
                };

                let server_login_start_result = ServerLogin::start(
                    &mut server_rng,
                    &self.opaque_server_setup,
                    Some(password_file),
                    credential_request,
                    r2ps_request.device_id.as_bytes(),
                    server_login_parameters,
                )
                .map_err(|e| {
                    warn!("error decoding pake request: {:?}", e);
                    ServiceRequestError::ServerLoginStartFailed
                })?;

                let credential_response_bytes = server_login_start_result.message.serialize();

                let pake_session_id = r2ps_request
                    .service_request
                    .pake_session_id
                    .clone()
                    .unwrap_or(Uuid::new_v4().to_string());
                self.pending_auth_spi_port.store_pending_auth(
                    pake_session_id.as_str(),
                    &Arc::new(LoginSession::new(server_login_start_result.state)),
                );
                let pake_response = PakeResponsePayload {
                    pake_session_id: Some(pake_session_id),
                    task: None,
                    response_data: Some(STANDARD.encode(credential_response_bytes)),
                    message: None,
                    session_expiration_time: None,
                };

                let elapsed = start.elapsed();
                info!("AUTH evaluate time: {} ns", elapsed.as_nanos());

                Ok(R2psResponse {
                    state: r2ps_request.state.clone(),
                    payload: ServiceResponse::Pake(pake_response),
                })
            }
            PakeState::Finalize => {
                let session = self
                    .pending_auth_spi_port
                    .get_pending_auth(
                        r2ps_request
                            .service_request
                            .pake_session_id
                            .clone()
                            .ok_or(ServiceRequestError::UnknownSession)?
                            .as_str(),
                    )
                    .ok_or(ServiceRequestError::InvalidAuthenticateRequest)?;

                let context = "RPS-Ops".as_bytes();
                let client = r2ps_request.device_id.as_bytes();
                let server = "https://cloud-wallet.digg.se/rhsm".as_bytes();
                let server_login_parameters = ServerLoginParameters {
                    context: Some(context),
                    identifiers: Identifiers {
                        client: Some(client),
                        server: Some(server),
                    },
                };
                let server_login = session
                    .take()
                    .ok_or(ServiceRequestError::InvalidAuthenticateRequest)?;
                let result = server_login
                    .finish(
                        CredentialFinalization::deserialize(&decoded_request_data)
                            .map_err(|_| ServiceRequestError::InvalidAuthenticateRequest)?,
                        server_login_parameters,
                    )
                    .map_err(|e| {
                        warn!("could not finish auth request request: {:?}", e);
                        ServiceRequestError::ServerLoginFinishFailed
                    })?;

                info!("SESSION KEY: {:X}", result.session_key);

                self.session_key_spi_port
                    .store(
                        r2ps_request
                            .service_request
                            .pake_session_id
                            .clone()
                            .unwrap()
                            .as_str(),
                        SessionKey::new(result.session_key.to_vec()),
                    )
                    .map_err(|_| ServiceRequestError::InternalServerError)?;

                let msg = br#"{"msg":"OK"}"#.to_vec();
                let pake_response = PakeResponsePayload {
                    pake_session_id: r2ps_request.service_request.pake_session_id.clone(),
                    task: None,
                    response_data: Some(STANDARD.encode(&msg)),
                    message: None,
                    session_expiration_time: Some(
                        (Utc::now() + chrono::Duration::minutes(15)).to_rfc3339(),
                    ),
                };

                let elapsed = start.elapsed();
                info!("AUTH finalize time: {} ns", elapsed.as_nanos());

                Ok(R2psResponse {
                    state: r2ps_request.state.clone(),
                    payload: ServiceResponse::Pake(pake_response),
                })
            }
        }
    }

    pub(crate) fn pin_registration(
        &self,
        r2ps_request: R2psRequest,
        decrypted_service_data: Option<DecryptedData>,
    ) -> Result<R2psResponse, ServiceRequestError> {
        let data =
            decrypted_service_data.ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let pake_payload = PakeRequestPayload::deserialize(data.to_vec()).map_err(|e| {
            warn!("error decoding pake registration request: {:?}", e);
            ServiceRequestError::InvalidPakeRequestPayload
        })?;

        info!(
            "deserialized pake payload req={}",
            pake_payload.request_data
        );

        // Handle Java jackson double base64 encoding....TODO remove later
        let decoded_request_data: Vec<u8> = general_purpose::STANDARD
            .decode(pake_payload.request_data)
            .map_err(|e| {
                warn!("error base64 decoding pake registration request: {:?}", e);
                ServiceRequestError::InvalidPakeRequestPayload
            })?;

        match pake_payload.state {
            PakeState::Evaluate => {
                let registration_request = RegistrationRequest::deserialize(&decoded_request_data)
                    .map_err(|e| {
                        warn!("invalid registration request evaluate: {:?}", e);
                        ServiceRequestError::InvalidRegistrationRequest
                    })?;

                let server_registration_start_result =
                    ServerRegistration::<DefaultCipherSuite>::start(
                        &self.opaque_server_setup,
                        registration_request,
                        r2ps_request.device_id.as_bytes(),
                    )
                    .map_err(|e| {
                        warn!("invalid registration request evaluate: {:?}", e);
                        ServiceRequestError::ServerRegistrationStartFailed
                    })?;

                info!(
                    "server_registration_start_result: {:?}",
                    server_registration_start_result.message
                );

                let response_data = server_registration_start_result
                    .message
                    .serialize()
                    .to_vec();
                let pake_response = PakeResponsePayload {
                    pake_session_id: None,
                    task: None,
                    response_data: Some(STANDARD.encode(response_data)),
                    message: None,
                    session_expiration_time: None,
                };

                Ok(R2psResponse {
                    state: r2ps_request.state,
                    payload: ServiceResponse::Pake(pake_response),
                })
            }
            PakeState::Finalize => {
                let registration_request: RegistrationUpload<DefaultCipherSuite> =
                    RegistrationUpload::deserialize(&decoded_request_data).map_err(|e| {
                        warn!("invalid registration request finalize: {:?}", e);
                        ServiceRequestError::InvalidRegistrationRequest
                    })?;

                let password_file =
                    ServerRegistration::<DefaultCipherSuite>::finish(registration_request);
                let password_file_serialized = password_file.serialize();
                info!("password file: {:?}", hex::encode(password_file_serialized));

                let new_state = DeviceHsmState {
                    client_id: r2ps_request.state.client_id,
                    wallet_id: r2ps_request.state.wallet_id,
                    client_public_key: r2ps_request.state.client_public_key,
                    password_file: Some(password_file_serialized),
                    keys: Vec::new(), // TODO this deletes all keys when wallet is registered
                };

                let msg = br#"{"msg":"OK"}"#.to_vec();
                let pake_response = PakeResponsePayload {
                    pake_session_id: None,
                    task: None,
                    response_data: Some(STANDARD.encode(&msg)),
                    message: None,
                    session_expiration_time: None,
                };

                Ok(R2psResponse {
                    state: new_state,
                    payload: ServiceResponse::Pake(pake_response),
                })
            }
        }
    }

    pub fn delete_key(
        &self,
        r2ps_request: R2psRequest,
        decrypted_service_data: Option<DecryptedData>,
    ) -> Result<R2psResponse, ServiceRequestError> {
        let data =
            decrypted_service_data.ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let payload = serde_json::from_slice::<DeleteKeyServiceData>(&data)
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;

        let new_state = DeviceHsmState {
            client_id: r2ps_request.state.client_id,
            wallet_id: r2ps_request.state.wallet_id,
            client_public_key: r2ps_request.state.client_public_key,
            password_file: r2ps_request.state.password_file,
            keys: r2ps_request
                .state
                .keys
                .into_iter()
                .filter(|key| key.public_key_jwk.kid != payload.kid)
                .collect(),
        };

        Ok(R2psResponse {
            state: new_state,
            payload: ServiceResponse::DeleteKey(DeleteKeyServiceData { kid: payload.kid }),
        })
    }

    pub fn hsm_ecdsa_sign(
        &self,
        r2ps_request: R2psRequest,
        decrypted_service_data: Option<DecryptedData>,
    ) -> Result<R2psResponse, ServiceRequestError> {
        let data =
            decrypted_service_data.ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let sign_request = serde_json::from_slice::<SignRequest>(&data)
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;

        let hsm_key = r2ps_request
            .state
            .keys
            .iter()
            .find(|key| key.public_key_jwk.kid.eq(&sign_request.kid))
            .cloned()
            .ok_or(ServiceRequestError::UnknownKey)?;

        let raw_sig_bytes = self
            .hsm_spi_port
            .sign(&hsm_key.wrapped_private_key, &sign_request.tbs_hash)
            .map_err(|_| ServiceRequestError::Unknown)?;
        let signature = p256::ecdsa::Signature::from_slice(&raw_sig_bytes)
            .map_err(|_| ServiceRequestError::Unknown)?;
        let asn1_signature: Vec<u8> = signature.to_der().as_bytes().to_vec();
        info!("Hsm Ecdsa asn1_signature: {:?}", asn1_signature);
        Ok(R2psResponse {
            state: r2ps_request.state,
            payload: ServiceResponse::Asn1Signature(asn1_signature),
        })
    }

    pub fn hsm_key_gen(
        &self,
        r2ps_request: R2psRequest,
        decrypted_service_data: Option<DecryptedData>,
    ) -> Result<R2psResponse, ServiceRequestError> {
        let data =
            decrypted_service_data.ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;
        let payload = serde_json::from_slice::<CreateKeyServiceData>(&data)
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;

        let hsm_key = self
            .hsm_spi_port
            .generate_key("foobar", &payload.curve)
            .map_err(|_| ServiceRequestError::Unknown)?;

        let mut new_keys = r2ps_request.state.keys.clone();
        new_keys.push(hsm_key.clone());

        let new_state = DeviceHsmState {
            client_id: r2ps_request.state.client_id,
            wallet_id: r2ps_request.state.wallet_id,
            client_public_key: r2ps_request.state.client_public_key,
            password_file: r2ps_request.state.password_file,
            keys: new_keys,
        };

        Ok(R2psResponse {
            state: new_state,
            payload: ServiceResponse::CreateKey(CreateKeyServiceDataResponse {
                public_key: hsm_key.public_key_jwk,
            }),
        })
    }

    pub fn hsm_list_wallet_keys(
        &self,
        r2ps_request: R2psRequest,
    ) -> Result<R2psResponse, ServiceRequestError> {
        // let decrypted_service_data = self.decrypt_service_data(&r2ps_request.service_request)?;

        let list_keys = ListKeysResponse {
            key_info: r2ps_request
                .state
                .keys
                .iter()
                .map(|key| KeyInfo {
                    public_key: key.public_key_jwk.clone(),
                    creation_time: Some(key.creation_time.clone()),
                })
                .collect(),
        };

        Ok(R2psResponse {
            state: r2ps_request.state,
            payload: ServiceResponse::ListKeys(list_keys),
        })
    }

    pub fn end_session(
        &self,
        r2ps_request: R2psRequest,
    ) -> Result<R2psResponse, ServiceRequestError> {
        self.session_key_spi_port
            .end_session(
                r2ps_request
                    .service_request
                    .pake_session_id
                    .clone()
                    .unwrap()
                    .as_str(),
            )
            .map_err(|_| ServiceRequestError::UnknownSession)?;

        let msg = br#"{"msg":"OK"}"#.to_vec();
        let pake_response = PakeResponsePayload {
            pake_session_id: r2ps_request.service_request.pake_session_id,
            task: None,
            response_data: Some(STANDARD.encode(&msg)),
            message: None,
            session_expiration_time: Some(Utc::now().to_rfc3339()),
        };

        Ok(R2psResponse {
            state: r2ps_request.state,
            payload: ServiceResponse::Pake(pake_response),
        })
    }

    pub(crate) fn process_service_request(
        &self,
        r2ps_request: R2psRequest,
        decrypted_service_data: Option<DecryptedData>,
    ) -> Result<R2psResponse, ServiceRequestError> {
        info!(
            "SERVICE TYPE REQUEST {:?}",
            r2ps_request.service_request.service_type
        );
        match r2ps_request.service_request.service_type {
            ServiceTypeId::Authenticate => self.authenticate(&r2ps_request, decrypted_service_data),
            ServiceTypeId::PinRegistration => {
                self.pin_registration(r2ps_request, decrypted_service_data)
            }
            ServiceTypeId::PinChange => Err(ServiceRequestError::Unknown),
            ServiceTypeId::HsmEcdsa => self.hsm_ecdsa_sign(r2ps_request, decrypted_service_data),
            ServiceTypeId::HsmEcdh => Err(ServiceRequestError::Unknown),
            ServiceTypeId::HsmEcKeygen => self.hsm_key_gen(r2ps_request, decrypted_service_data),
            ServiceTypeId::HsmEcDeleteKey => self.delete_key(r2ps_request, decrypted_service_data),
            ServiceTypeId::HsmListKeys => self.hsm_list_wallet_keys(r2ps_request),
            ServiceTypeId::SessionEnd => self.end_session(r2ps_request),
            ServiceTypeId::SessionContextEnd => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Store => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Retrieve => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Log => Err(ServiceRequestError::Unknown),
            ServiceTypeId::GetLog => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Info => Err(ServiceRequestError::Unknown),
        }
    }

    fn decrypt_service_data(
        &self,
        service_request: &ServiceRequest,
        session_key: Option<&SessionKey>,
    ) -> Result<DecryptedData, ServiceRequestError> {
        let decrypted_service_data = match service_request.service_type.encrypt_option() {
            EncryptOption::User => {
                let session_key = session_key
                    .ok_or(R2psRequestError::UnknownSession)
                    .map_err(|_| ServiceRequestError::UnknownSession)?;

                self.decrypt_jwe(
                    &service_request
                        .clone()
                        .service_data
                        .ok_or(ServiceRequestError::Unknown)?, // TODO
                    session_key,
                )
                .map_err(|_| ServiceRequestError::JweError)?
            }
            EncryptOption::Device => decrypt_service_data_jwe(
                service_request,
                &self.r2ps_server_config.server_private_key,
            )
            .map_err(|e| {
                error!("Could not decrypt service data: {:?}", e);
                ServiceRequestError::JweError
            })?,
        };
        Ok(decrypted_service_data)
    }
}

impl R2psRequestUseCase for R2psService {
    fn execute(&self, r2ps_request_jws: R2psRequestJws) -> Result<R2psRequestId, R2psRequestError> {
        let state = decode_state_jws(
            r2ps_request_jws.state_jws,
            &self.r2ps_server_config.server_public_key,
        )
        .map_err(|_| R2psRequestError::InvalidState)?;

        let client_public_key = ec_jwk_to_pem(&state.client_public_key).map_err(|_| {
            R2psRequestError::ServiceError(ServiceRequestError::InvalidClientPublicKey)
        })?;
        let service_request =
            decode_service_request_jws(r2ps_request_jws.service_request_jws, &client_public_key)
                .map_err(|_| R2psRequestError::JwsError)?;

        debug!("DECODED JWS {:?}", service_request);

        if service_request.context != "hsm" {
            return Err(R2psRequestError::UnsupportedContext);
        }

        if let Some(pake_session_id) = &service_request.pake_session_id {
            debug!("pake_session_id: {:?}", pake_session_id);
        }

        let session_key = service_request
            .pake_session_id
            .as_ref()
            .and_then(|id| self.session_key_spi_port.get(id));

        let decrypted_service_data = if service_request.service_data.is_some() {
            Some(
                self.decrypt_service_data(&service_request, session_key.as_ref())
                    .map_err(R2psRequestError::ServiceError)?,
            )
        } else {
            None
        };

        let r2ps_response = self
            .process_service_request(
                R2psRequest {
                    request_id: r2ps_request_jws.request_id.clone(),
                    wallet_id: r2ps_request_jws.wallet_id.clone(),
                    device_id: r2ps_request_jws.device_id.clone(),
                    state: state.clone(),
                    service_request: service_request.clone(),
                },
                decrypted_service_data,
            )
            .map_err(R2psRequestError::ServiceError)?;

        let enc_option = service_request.service_type.encrypt_option();
        info!(
            "Response to {:?} will be encrypted with {:?} encryption",
            service_request.service_type, enc_option
        );

        let response_payload = r2ps_response
            .payload
            .serialize()
            .map_err(|_| R2psRequestError::EncryptionError)?;

        match response_payload.first() == Some(&b'{') && response_payload.last() == Some(&b'}') {
            true => debug!(
                "JSON Response payload before encryption: {}",
                String::from_utf8_lossy(&response_payload)
            ),
            false => debug!(
                "Response payload before encryption (hex): {:02X?}",
                r2ps_response
            ),
        }

        let new_state_jws =
            encode_state_jws(&r2ps_response.state, None).map_err(|_| R2psRequestError::JwsError)?;
        let jwe = match enc_option {
            EncryptOption::User => {
                let session_key = session_key
                    .clone()
                    .ok_or(R2psRequestError::UnknownSession)?;
                self.encrypt_with_aes(&response_payload, &session_key)
                    .map_err(|_| R2psRequestError::EncryptionError)?
            }
            EncryptOption::Device => encrypt_with_ec_pem(
                &response_payload,
                &ec_jwk_to_pem(&state.client_public_key)
                    .map_err(|_| R2psRequestError::EncryptionError)?,
            )
            .map_err(|_| R2psRequestError::EncryptionError)?,
        };

        let jws = jws_with_jwk(
            &jwe,
            service_request.nonce,
            service_request.service_type.encrypt_option(),
        )
        .map_err(|_| R2psRequestError::JwsError)?;

        info!(
            "JWS response payload on {:?} {}",
            service_request.service_type, jws
        );

        let r2ps_response_jws = R2psResponseJws {
            request_id: r2ps_request_jws.request_id.clone(),
            wallet_id: r2ps_request_jws.wallet_id.clone(),
            device_id: r2ps_request_jws.device_id.clone(),
            http_status: 200,
            state_jws: new_state_jws,
            service_response_jws: jws,
        };

        self.r2ps_response_spi_port
            .send(r2ps_response_jws.clone())
            .map(|_| r2ps_response_jws.request_id.clone())
            .map_err(|_| R2psRequestError::ConnectionError)
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

fn encrypt_with_ec_pem(
    payload: &[u8],
    client_public_key: &Pem,
) -> Result<String, ServiceRequestError> {
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");

    let pem_string = pem::encode(client_public_key);
    match ECDH_ES.encrypter_from_pem(&pem_string) {
        Ok(encrypter) => match jwe::serialize_compact(payload, &header, &encrypter) {
            Ok(payload_bytes) => Ok(payload_bytes),
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

// todo remove logging of sensitive info
pub fn decrypt_service_data_jwe(
    service_request: &ServiceRequest,
    server_private_key: &Pem,
) -> Result<DecryptedData, ServiceRequestError> {
    let service_data = service_request
        .service_data
        .as_ref()
        .ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;

    info!("SERVICE DATA ******* {} ", service_data);

    let decoded_string = String::from_utf8(BASE64_STANDARD.decode(service_data)?)?;
    let decrypter = ECDH_ES.decrypter_from_pem(pem::encode(server_private_key))?;
    let (payload, _) = jwe::deserialize_compact(&decoded_string, &decrypter)?;

    info!("decrypted JWS payload: {}", hex::encode(&payload));

    if let Ok(text) = String::from_utf8(payload.clone()) {
        info!("decrypted JWS payload: {}", text);
    }

    Ok(DecryptedData::new(payload))
}

impl EncryptOption {
    fn as_str(&self) -> &'static str {
        match self {
            EncryptOption::User => "user",
            EncryptOption::Device => "device",
        }
    }
}

fn jws_with_jwk(
    data: &str,
    nonce: Option<String>,
    enc: EncryptOption,
) -> Result<String, ServiceRequestError> {
    let claims = Claims {
        ver: "1.0".to_string(),
        nonce: nonce.unwrap().to_string(),
        iat: Utc::now().to_rfc3339(),
        enc: enc.as_str().to_string(),
        data: STANDARD.encode(data),
    };
    let mut header = Header::new(Algorithm::ES256);
    header.typ = Some("JOSE".to_string());

    let private_key_pem = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg/NIIdRGO+qU2bjxT
tnZuC45gAg6wZ0UGe9nCeM7wc0yhRANCAASnNDG5ct6I/LOK0wpBtRJU4PcDFv6X
0upWOzkadhqcDWTgCYxROhakhPDldczjw0+FuAyGgzQVSng5DbrP+8JB
-----END PRIVATE KEY-----"#;

    let encoding_key = EncodingKey::from_ec_pem(private_key_pem.as_bytes()).unwrap();

    let token = encode(&header, &claims, &encoding_key).unwrap();

    println!("JWS Token: {}", token);
    Ok(token)
}

fn encode_state_jws(
    state: &DeviceHsmState,
    nonce: Option<String>,
) -> Result<String, ServiceRequestError> {
    let mut header = Header::new(Algorithm::ES256);
    header.typ = Some("JOSE".to_string());

    let private_key_pem = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg/NIIdRGO+qU2bjxT
tnZuC45gAg6wZ0UGe9nCeM7wc0yhRANCAASnNDG5ct6I/LOK0wpBtRJU4PcDFv6X
0upWOzkadhqcDWTgCYxROhakhPDldczjw0+FuAyGgzQVSng5DbrP+8JB
-----END PRIVATE KEY-----"#;

    let encoding_key = EncodingKey::from_ec_pem(private_key_pem.as_bytes()).unwrap();

    let token = encode(&header, &state, &encoding_key).unwrap();

    println!("JWS Token: {}", token);
    Ok(token)
}

fn decode_state_jws(
    state_jws: String,
    public_key: &Pem,
) -> Result<DeviceHsmState, ServiceRequestError> {
    let pem_string = pem::encode(public_key);

    match DecodingKey::from_ec_pem(pem_string.as_bytes()) {
        Ok(decoding_key) => {
            let mut validation = Validation::new(Algorithm::ES256);
            validation.validate_exp = false; // TODO: signera om state regelbundet?
            validation.required_spec_claims.clear();
            validation.insecure_disable_signature_validation();
            match decode::<DeviceHsmState>(&state_jws, &decoding_key, &validation) {
                Ok(service_request_claims) => {
                    info!("decoded claims: {:?}", service_request_claims);
                    Ok(service_request_claims.claims)
                }
                Err(error) => {
                    error!("Error decoding jws claims: {:?}", error);
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
fn decode_service_request_jws(
    service_request_jws: String,
    client_public_key: &Pem,
) -> Result<ServiceRequest, ServiceRequestError> {
    let pem_string = pem::encode(client_public_key);

    match DecodingKey::from_ec_pem(pem_string.as_bytes()) {
        Ok(decoding_key) => {
            let mut validation = Validation::new(Algorithm::ES256);
            validation.validate_exp = false; // TODO kolla om vi ska validera
            validation.required_spec_claims.clear();
            match decode::<ServiceRequest>(&service_request_jws, &decoding_key, &validation) {
                Ok(service_request_claims) => {
                    info!("decoded claims: {:?}", service_request_claims);
                    Ok(service_request_claims.claims)
                }
                Err(error) => {
                    error!("Error decoding jws claims: {:?}", error);
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
