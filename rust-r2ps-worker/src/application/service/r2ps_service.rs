use crate::application::client_repository_spi_port::ClientRepositorySpiPort;
use crate::application::device_permit_list_spi_port::DevicePermitListSpiPort;
use crate::application::hsm_spi_port::HsmSpiPort;
use crate::application::pending_auth_spi_port::{LoginSession, PendingAuthSpiPort};
use crate::application::session_key_spi_port::SessionKeySpiPort;
use crate::application::{
    R2psRequestId, R2psRequestUseCase, R2psResponseSpiPort, load_pem_from_bas64_env,
};
use crate::domain::value_objects::r2ps::{
    Claims, PakeRequestPayload, PakeResponsePayload, ServiceRequest,
};
use crate::domain::{
    ClientMetadata, CreateKeyServiceData, CreateKeyServiceDataResponse, DefaultCipherSuite,
    EncryptOption, KeyInfo, ListKeysResponse, PakeState, R2PsResponse, R2psRequest,
    R2psRequestError, R2psServerConfig, ServiceRequestError, ServiceTypeId, SignRequest,
};
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

#[derive(Clone)]
pub struct R2psService {
    r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
    hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
    opaque_server_setup: ServerSetup<DefaultCipherSuite>,
    client_repository_spi_port: Arc<dyn ClientRepositorySpiPort + Send + Sync>,
    r2ps_server_config: R2psServerConfig,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
    device_permit_list_spi_port: Arc<dyn DevicePermitListSpiPort + Send + Sync>,
}

impl R2psService {
    pub fn new(
        r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
        client_repository_spi_port: Arc<dyn ClientRepositorySpiPort + Send + Sync>,
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
        hsm_spi_port: Arc<dyn HsmSpiPort + Send + Sync>,
        pending_auth_spi_port: Arc<dyn PendingAuthSpiPort + Send + Sync>,
        device_permit_list_spi_port: Arc<dyn DevicePermitListSpiPort + Send + Sync>,
    ) -> Self {
        let server_public_key =
            load_pem_from_bas64_env("SERVER_PUBLIC_KEY").expect("Failed to load SERVER_PUBLIC_KEY");
        let server_private_key = load_pem_from_bas64_env("SERVER_PRIVATE_KEY")
            .expect("Failed to load SERVER_PRIVATE_KEY");
        let server_setup =
            create_server_setup(&server_private_key).expect("Failed to create opaque server setup");

        Self {
            r2ps_response_spi_port,
            client_repository_spi_port,
            session_key_spi_port,
            hsm_spi_port,
            opaque_server_setup: server_setup,
            r2ps_server_config: R2psServerConfig {
                server_public_key,
                server_private_key,
            },
            pending_auth_spi_port,
            device_permit_list_spi_port,
        }
        // TODO
        //let mut registered_users =
        //    HashMap::<String, GenericArray<u8, ServerRegistrationLen<DefaultCipherSuite>>>::new();
        //registered_users.insert("a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(), Default::default());
    }

    pub fn encrypt_with_aes(
        &self,
        payload: &Vec<u8>,
        pake_session_id: &str,
    ) -> Result<String, ServiceRequestError> {
        match self.session_key_spi_port.get(pake_session_id) {
            Some(session_key) => {
                let mut header = JweHeader::new();
                header.set_algorithm("dir");
                header.set_content_encryption("A256GCM");

                let encrypter = DirectJweAlgorithm::Dir
                    .encrypter_from_bytes(session_key)
                    .map_err(|e| {
                        error!("Failed to create encrypter: {:?}", e);
                        ServiceRequestError::Unknown
                    })?;

                jwe::serialize_compact(payload, &header, &encrypter).map_err(|e| {
                    error!("Failed to encrypt: {:?}", e);
                    ServiceRequestError::Unknown
                })
            }
            None => Err(ServiceRequestError::Unknown),
        }
    }
    pub fn decrypt_jwe(
        &self,
        encrypted_payload: &str,
        pake_session_id: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        match self.session_key_spi_port.get(pake_session_id) {
            Some(session_key) => match BASE64_STANDARD.decode(&encrypted_payload) {
                Ok(data) => {
                    info!("decoded service_data hex: {:02X?}", data);
                    match String::from_utf8(data) {
                        Ok(decoded_string) => {
                            info!("decoded service_data utf8: {}", decoded_string);
                            info!("decrypt with session key {:02X?}", session_key);
                            let decrypter = DirectJweAlgorithm::Dir
                                .decrypter_from_bytes(session_key.to_bytes())?;
                            let (payload, _header) =
                                josekit::jwe::deserialize_compact(&decoded_string, &decrypter)?;
                            Ok(payload)
                        }
                        Err(_) => Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "No session key",
                        ))),
                    }
                }
                Err(_) => Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "No session key",
                ))),
            },
            None => Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "No session key",
            ))),
        }
    }

    pub(crate) fn authenticate(
        &self,
        decrypted_payload: &Vec<u8>,
        device_id: &str,
        r2ps_service: &R2psService,
        pake_session_id: &String,
    ) -> Result<Vec<u8>, ServiceRequestError> {
        let start = Instant::now();

        let pake_payload = PakeRequestPayload::deserialize(&decrypted_payload).map_err(|e| {
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
                let client_metadata = r2ps_service
                    .client_repository_spi_port
                    .client_metadata(device_id);
                let password_file_serialized = client_metadata
                    .and_then(|meta_data| meta_data.password_file.clone())
                    .ok_or_else(|| ServiceRequestError::UnknownClient)?;

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
                let client = device_id.as_bytes();
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
                    &r2ps_service.opaque_server_setup,
                    Some(password_file),
                    credential_request,
                    device_id.as_bytes(),
                    server_login_parameters,
                )
                .map_err(|e| {
                    warn!("error decoding pake request: {:?}", e);
                    ServiceRequestError::ServerLoginStartFailed
                })?;

                let credential_response_bytes = server_login_start_result.message.serialize();
                let session = Arc::new(LoginSession::new(server_login_start_result.state));
                self.pending_auth_spi_port
                    .store_pending_auth(&pake_session_id, &session);
                let pake_response = PakeResponsePayload {
                    pake_session_id: Some(pake_session_id.to_string()),
                    task: None,
                    response_data: Some(
                        general_purpose::STANDARD.encode(credential_response_bytes.to_vec()),
                    ),
                    message: None,
                    session_expiration_time: None,
                };

                let elapsed = start.elapsed();
                info!("AUTH evaluate time: {} ns", elapsed.as_nanos());

                match serde_json::to_vec(&pake_response) {
                    Ok(payload_vec) => Ok(payload_vec),
                    Err(_) => Err(ServiceRequestError::Unknown),
                }
            }
            PakeState::Finalize => {
                let session = self
                    .pending_auth_spi_port
                    .get_pending_auth(&pake_session_id)
                    .ok_or(ServiceRequestError::InvalidAuthenticateRequest)?;

                let context = "RPS-Ops".as_bytes();
                let client = device_id.as_bytes();
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
                    .store(&pake_session_id, result.session_key.to_vec())
                    .map_err(|_| ServiceRequestError::InternalServerError)?;

                let msg = br#"{"msg":"OK"}"#.to_vec();
                let pake_response = PakeResponsePayload {
                    pake_session_id: Some(pake_session_id.to_string()),
                    task: None,
                    response_data: Some(general_purpose::STANDARD.encode(msg.to_vec())),
                    message: None,
                    session_expiration_time: Some(Utc::now().timestamp_millis()),
                };

                let elapsed = start.elapsed();
                info!("AUTH finalize time: {} ns", elapsed.as_nanos());

                serde_json::to_vec(&pake_response).map_err(|e| {
                    error!("Could not serialize authenticate response: {:?}", e);
                    ServiceRequestError::SerializeResponseError
                })
            }
        }
    }

    pub(crate) fn pin_registration(
        &self,
        decrypted_payload: &Vec<u8>,
        device_id: &str,
        r2ps_service: &R2psService,
    ) -> Result<Vec<u8>, ServiceRequestError> {
        let pake_payload = PakeRequestPayload::deserialize(&decrypted_payload).map_err(|e| {
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
                        &r2ps_service.opaque_server_setup,
                        registration_request,
                        device_id.as_bytes(),
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
                    response_data: Some(general_purpose::STANDARD.encode(response_data.to_vec())),
                    message: None,
                    session_expiration_time: None,
                };

                serde_json::to_vec(&pake_response).map_err(|e| {
                    warn!("Could not serialize pake response payload: {:?}", e);
                    ServiceRequestError::SerializeResponseError
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

                match r2ps_service
                    .client_repository_spi_port
                    .client_metadata(device_id)
                {
                    Some(client_metadata) => {
                        let _ = r2ps_service.client_repository_spi_port.store_metadata(
                            ClientMetadata {
                                client_id: client_metadata.client_id,
                                wallet_id: client_metadata.wallet_id,
                                client_public_key: client_metadata.client_public_key,
                                password_file: Some(password_file_serialized),
                                keys: Vec::new(), // TODO this deletes all keys when wallet is registered
                            },
                        );
                        info!("Store metadata: {:?}", password_file);
                    }
                    None => {
                        // TODO register new?
                    }
                }

                let msg = br#"{"msg":"OK"}"#.to_vec();
                let pake_response = PakeResponsePayload {
                    pake_session_id: None,
                    task: None,
                    response_data: Some(general_purpose::STANDARD.encode(msg.to_vec())),
                    message: None,
                    session_expiration_time: None,
                };

                match serde_json::to_vec(&pake_response) {
                    Ok(payload_vec) => Ok(payload_vec),
                    Err(_) => Err(ServiceRequestError::Unknown),
                }
            }
        }
    }

    pub fn hsm_ecdsa_sign(
        &self,
        decrypted_payload: &Vec<u8>,
        device_id: &str,
    ) -> Result<Vec<u8>, ServiceRequestError> {
        let payload = serde_json::from_slice::<SignRequest>(&decrypted_payload)
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;
        let metadata = self
            .client_repository_spi_port
            .client_metadata(device_id)
            .ok_or(ServiceRequestError::UnknownClient)?;

        let hsm_key = metadata
            .keys
            .iter()
            .find(|key| key.kid.eq(&payload.kid))
            .ok_or(ServiceRequestError::UnknownKey)?;

        let raw_sig_bytes = self
            .hsm_spi_port
            .sign(&hsm_key.wrapped_private_key, &payload.tbs_hash.into_bytes())
            .map_err(|_| ServiceRequestError::Unknown)?;
        let signature = p256::ecdsa::Signature::from_slice(&raw_sig_bytes)
            .map_err(|_| ServiceRequestError::Unknown)?;
        let asn1_signature: Vec<u8> = signature.to_der().as_bytes().to_vec();
        info!("Hsm Ecdsa asn1_signature: {:?}", asn1_signature);
        Ok(asn1_signature)
    }

    pub fn hsm_key_gen(
        &self,
        decrypted_payload: &Vec<u8>,
        device_id: &str,
    ) -> Result<Vec<u8>, ServiceRequestError> {
        let payload = serde_json::from_slice::<CreateKeyServiceData>(&decrypted_payload)
            .map_err(|_| ServiceRequestError::InvalidServiceRequestFormat)?;

        let key = self
            .hsm_spi_port
            .generate_key("foobar", &payload.curve)
            .map_err(|_| ServiceRequestError::Unknown)?;

        // TODO ändra protokollet så att t.ex. id och publik nyckel returneras???
        self.client_repository_spi_port
            .add_key(&device_id, &key)
            .map_err(|_| ServiceRequestError::Unknown)?;

        serde_json::to_vec(&CreateKeyServiceDataResponse {
            created_key: payload.curve,
        })
        .map_err(|_| ServiceRequestError::SerializeResponseError)
    }

    pub fn hsm_list_wallet_keys(&self, device_id: &str) -> Result<Vec<u8>, ServiceRequestError> {
        let keys_response = match self.client_repository_spi_port.client_metadata(device_id) {
            None => {
                println!("No metadata");
                ListKeysResponse {
                    key_info: Vec::new(),
                }
            }
            Some(metadata) => ListKeysResponse {
                key_info: metadata
                    .keys
                    .iter()
                    .map(|key| KeyInfo {
                        kid: key.kid.clone(),
                        public_key: key
                            .public_key_pem
                            .clone()
                            .lines()
                            .filter(|line| !line.starts_with("-----"))
                            .collect::<Vec<_>>()
                            .join(""),
                        curve_name: key.curve_name.clone(),
                        creation_time: Some(key.creation_time.timestamp_millis()),
                    })
                    .collect(),
            },
        };
        serde_json::to_vec(&keys_response).map_err(|_| ServiceRequestError::SerializeResponseError)
    }

    pub fn end_session(&self, pake_session_id: &str) -> Result<Vec<u8>, ServiceRequestError> {
        self.session_key_spi_port
            .end_session(&pake_session_id.to_string())
            .map_err(|_| ServiceRequestError::UnknownSession)?;

        let msg = br#"{"msg":"OK"}"#.to_vec();
        let pake_response = PakeResponsePayload {
            pake_session_id: Some(pake_session_id.to_string()),
            task: None,
            response_data: Some(general_purpose::STANDARD.encode(msg.to_vec())),
            message: None,
            session_expiration_time: Some(Utc::now().timestamp_millis()),
        };

        serde_json::to_vec(&pake_response).map_err(|_| ServiceRequestError::SerializeResponseError)
    }

    pub(crate) fn process_service_request(
        &self,
        service_request: &ServiceRequest,
        decrypted_payload: &Vec<u8>,
        device_id: &str,
        r2ps_service: &R2psService,
    ) -> Result<Vec<u8>, ServiceRequestError> {
        let pake_session_id = match &service_request.pake_session_id {
            Some(session_id) => session_id.to_string(),
            None => Uuid::new_v4().to_string(),
        };
        info!("SERVICE TYPE REQUEST {:?}", service_request.service_type);
        match service_request.service_type {
            ServiceTypeId::Authenticate => self.authenticate(
                decrypted_payload,
                device_id,
                &r2ps_service,
                &pake_session_id,
            ),
            ServiceTypeId::PinRegistration => {
                self.pin_registration(decrypted_payload, device_id, &r2ps_service)
            }
            ServiceTypeId::PinChange => Err(ServiceRequestError::Unknown),
            ServiceTypeId::HsmEcdsa => self.hsm_ecdsa_sign(decrypted_payload, device_id),
            ServiceTypeId::HsmEcdh => Err(ServiceRequestError::Unknown),
            ServiceTypeId::HsmEcKeygen => self.hsm_key_gen(decrypted_payload, device_id),
            ServiceTypeId::HsmEcDeleteKey => Err(ServiceRequestError::Unknown),
            ServiceTypeId::HsmListKeys => self.hsm_list_wallet_keys(device_id),
            ServiceTypeId::SessionEnd => self.end_session(&pake_session_id),
            ServiceTypeId::SessionContextEnd => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Store => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Retrieve => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Log => Err(ServiceRequestError::Unknown),
            ServiceTypeId::GetLog => Err(ServiceRequestError::Unknown),
            ServiceTypeId::Info => Err(ServiceRequestError::Unknown),
        }
    }
    pub fn decode_r2ps_request_jws(
        &self,
        input: &R2psRequest,
        client_public_key: &Pem,
    ) -> Result<ServiceRequest, ServiceRequestError> {
        let pem_string = pem::encode(&client_public_key);

        match DecodingKey::from_ec_pem(pem_string.as_bytes()) {
            Ok(decoding_key) => {
                let mut validation = Validation::new(Algorithm::ES256);
                validation.validate_exp = false; // Your token doesn't have 'exp'
                validation.required_spec_claims.clear();
                match decode::<ServiceRequest>(&input.payload, &decoding_key, &validation) {
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
}

impl R2psRequestUseCase for R2psService {
    fn execute(&self, r2ps_request: R2psRequest) -> Result<R2psRequestId, R2psRequestError> {
        let client_metadata = self
            .client_repository_spi_port
            .client_metadata(r2ps_request.device_id.as_str())
            .ok_or(R2psRequestError::UnknownClient)?;

        let service_request = self
            .decode_r2ps_request_jws(&r2ps_request, &client_metadata.client_public_key)
            .map_err(|_| R2psRequestError::JwsError)?;

        debug!("DECODED JWS {:?}", service_request);

        if service_request.context != "hsm" {
            return Err(R2psRequestError::UnsupportedContext);
        }

        if let Some(pake_session_id) = &service_request.pake_session_id {
            // TODO: identifies session key for request
            debug!("pake_session_id: {:?}", pake_session_id);
            //return Err(R2psRequestError::NotImplemented);
        }

        let decrypted_payload = match service_request.service_type.encrypt_option() {
            EncryptOption::User => self
                .decrypt_jwe(
                    &service_request.clone().service_data.unwrap(),
                    &service_request.clone().pake_session_id.unwrap(),
                )
                .map_err(|_| R2psRequestError::DecryptionError)?,
            EncryptOption::Device => decrypt_service_data_jwe(
                &service_request,
                &self.r2ps_server_config.server_private_key,
            )
            .map_err(|e| {
                error!("Could not decrypt service data: {:?}", e);
                R2psRequestError::DecryptionError
            })?,
        };

        let response = self
            .process_service_request(
                &service_request,
                &decrypted_payload,
                &r2ps_request.device_id,
                self,
            )
            .map_err(|e| R2psRequestError::ServiceError(e))?;

        let jwe = match service_request.service_type.encrypt_option() {
            EncryptOption::User => {
                info!(
                    "user encrypted, aes encrypt response data with session key: {:?}",
                    response
                );
                self.encrypt_with_aes(&response, &service_request.clone().pake_session_id.unwrap())
                    .map_err(|_| R2psRequestError::EncryptionError)?
            }
            EncryptOption::Device => {
                info!("device encrypted, encrypt response data: {:?}", response);
                encrypt_with_ec_pem(&response, &client_metadata.client_public_key)
                    .map_err(|_| R2psRequestError::EncryptionError)?
            }
        };

        let jws =
            jws_with_jwk(&jwe, service_request.nonce).map_err(|_| R2psRequestError::JwsError)?;

        info!(
            "JWS response payload on {:?} {}",
            service_request.service_type, jws
        );

        let r2ps_response = R2PsResponse {
            request_id: r2ps_request.request_id.clone(),
            wallet_id: r2ps_request.wallet_id.clone(),
            device_id: r2ps_request.device_id.clone(),
            http_status: 200,
            payload: jws,
        };

        self.r2ps_response_spi_port
            .send(r2ps_response)
            .map(|_| r2ps_request.request_id.clone())
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
    payload: &Vec<u8>,
    client_public_key: &Pem,
) -> Result<String, ServiceRequestError> {
    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");

    let pem_string = pem::encode(&client_public_key);
    match ECDH_ES.encrypter_from_pem(&pem_string) {
        Ok(encrypter) => match josekit::jwe::serialize_compact(&payload, &header, &encrypter) {
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
) -> Result<Vec<u8>, ServiceRequestError> {
    let service_data = service_request
        .service_data
        .as_ref()
        .ok_or(ServiceRequestError::InvalidServiceRequestFormat)?;

    info!("SERVICE DATA ******* {} ", service_data);

    let decoded_string = String::from_utf8(BASE64_STANDARD.decode(service_data)?)?;
    let decrypter = ECDH_ES.decrypter_from_pem(&pem::encode(server_private_key))?;
    let (payload, _) = jwe::deserialize_compact(&decoded_string, &decrypter)?;

    info!("decrypted JWS payload: {}", hex::encode(&payload));

    if let Ok(text) = String::from_utf8(payload.clone()) {
        info!("decrypted JWS payload: {}", text);
    }

    Ok(payload)
}

fn encrypt_with_ec_jwk(
    payload: &PakeResponsePayload,
    ec_public_jwk: &josekit::jwk::Jwk,
) -> Result<String, Box<dyn std::error::Error>> {
    let payload_bytes = serde_json::to_vec(payload)?;

    let mut header = JweHeader::new();
    header.set_algorithm("ECDH-ES");
    header.set_content_encryption("A256GCM");

    let encrypter = ECDH_ES.encrypter_from_jwk(ec_public_jwk)?;
    let jwe = josekit::jwe::serialize_compact(&payload_bytes, &header, &encrypter)?;

    Ok(jwe)
}

fn jws_with_jwk(data: &str, nonce: Option<String>) -> Result<String, ServiceRequestError> {
    let now = Utc::now(); // Get duration in ms since Unix epoch
    let claims = Claims {
        ver: "1.0".to_string(),
        nonce: nonce.unwrap().to_string(),
        iat: now.timestamp(),
        enc: "device".to_string(),
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
