use crate::application::client_repository_spi_port::ClientRepositorySpiPort;
use crate::application::session_key_spi_port::{LoginSession, SessionKeySpiPort};
use crate::application::{load_pem_from_bas64_env, R2psRequestId, R2psRequestUseCase, R2psResponseSpiPort};
use crate::domain::value_objects::r2ps::{Claims, PakeRequestPayload, PakeResponsePayload, ServiceRequest};
use crate::domain::{ClientMetadata, PakeState, R2PsResponse, R2psRequest, R2psRequestError, R2psServerConfig, ServiceRequestError, ServiceTypeId};
use crate::DefaultCipherSuite;
use argon2::password_hash::rand_core::OsRng;
use base64::engine::general_purpose;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use josekit::jwe::{JweHeader, ECDH_ES};
use josekit::jwk::{Jwk};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use opaque_ke::{CredentialFinalization, CredentialRequest, Identifiers, RegistrationRequest, RegistrationUpload, ServerLogin, ServerLoginParameters, ServerLoginStartResult, ServerRegistration, ServerRegistrationLen, ServerSetup};
use pem::Pem;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use base64::engine::general_purpose::STANDARD;
use chrono::Utc;
use generic_array::GenericArray;
use opaque_ke::keypair::{KeyPair, OprfSeed, PrivateKey, PublicKey};
use p256::NistP256;
use rdkafka::message::ToBytes;
use tracing::{debug, error, info, instrument};
use strum_macros::Display;
use uuid::Uuid;
use p256::pkcs8::DecodePrivateKey;
use sha2::{Sha256, Digest};
use digest::Output;
use p256::elliptic_curve::sec1::ToEncodedPoint;

#[derive(Clone)]
pub struct R2psService {
    r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
    opaque_server_setup: ServerSetup<DefaultCipherSuite>,
    client_repository_spi_port: Arc<dyn ClientRepositorySpiPort + Send + Sync>,
    r2ps_server_config: R2psServerConfig,
    session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
}

impl R2psService {
    pub fn new(
        r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
        client_repository_spi_port: Arc<dyn ClientRepositorySpiPort + Send + Sync>,
        session_key_spi_port: Arc<dyn SessionKeySpiPort + Send + Sync>,
    ) -> Self {



        match (load_pem_from_bas64_env("SERVER_PUBLIC_KEY"),
               load_pem_from_bas64_env("SERVER_PRIVATE_KEY")) {
            (Ok(server_public_key), Ok(server_private_key)) => {
                let mut rng = OsRng;

                // 1. Parse P-256 private key from PEM
                let secret_key = p256::SecretKey::from_pkcs8_pem(&pem::encode(&server_private_key)).unwrap();

                // 2. Get raw private key bytes
                let private_bytes = secret_key.to_bytes();

                // 3. Get public key in UNCOMPRESSED SEC1 format (65 bytes: 0x04 || x || y)
                let public_key_point = secret_key.public_key();
                let public_encoded = public_key_point.as_affine().to_encoded_point(true); // false = uncompressed
                let public_bytes = public_encoded.as_bytes();

                println!("Private key length: {}", private_bytes.len());
                println!("Public key length: {}", public_bytes.len());
                println!("Public key first byte: 0x{:02x}", public_bytes[0]);

                // 4. Create opaque-ke key types
                let opaque_private_key = PrivateKey::<NistP256>::deserialize(&private_bytes)
                    .map_err(|e| format!("Failed to deserialize private key: {:?}", e)).unwrap();
                let opaque_public_key = PublicKey::<NistP256>::deserialize(public_bytes)
                    .map_err(|e| format!("Failed to deserialize public key: {:?}", e)).unwrap();

                // 5. Create KeyPair
                let keypair = KeyPair::new(opaque_private_key, opaque_public_key);

                let mut hasher = Sha256::new();
                hasher.update(keypair.public().serialize().to_vec());
                let result = hasher.finalize();
                info!("######## OPAQUE public key: {} {:?} {:?}", hex::encode(result), result, &keypair.public().serialize().to_vec());


                // 6. Create OPRF seed - needs to be Output<Sha256> (32 bytes)
                //let seed_hash: Output<Sha256> = Sha256::digest("a27366b536549dc6630f719bbcbaa16cbf70253d273640d7690f6e2e4ef6a5c7".as_bytes());
               // let oprf_seed = OprfSeed::<Sha256>::new();

                let server_setup = ServerSetup::<DefaultCipherSuite>::new_with_key_pair(
                    &mut rng,
                    keypair,
                );



                Self {
                    r2ps_response_spi_port,
                    client_repository_spi_port,
                    session_key_spi_port,
                    opaque_server_setup: server_setup,
                    r2ps_server_config: R2psServerConfig {
                        server_public_key,
                        server_private_key
                    }
                }
            }
            _ => {
                panic!("Invalid config")
            }
        }
        // TODO
        //let mut registered_users =
        //    HashMap::<String, GenericArray<u8, ServerRegistrationLen<DefaultCipherSuite>>>::new();
        //registered_users.insert("a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(), Default::default());


    }


}



impl R2psRequestUseCase for R2psService {

    fn execute(&self, r2ps_request: R2psRequest) -> Result<R2psRequestId, R2psRequestError> {
        let client_metadata = self
            .client_repository_spi_port
            .client_metadata(r2ps_request.device_id.as_str())
            .ok_or(R2psRequestError::UnknownClient)?;

        let service_request = decode_r2ps_request_jws(&r2ps_request, &client_metadata.client_public_key)
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

        let decrypted_payload = decrypt_service_data_jwe(
            &service_request,
            &self.r2ps_server_config.server_private_key,
        ).map_err(|_| R2psRequestError::DecryptionError)?;

        let response = process_service_request(
            &service_request,
            &decrypted_payload,
            &r2ps_request.device_id,
            self,
        ).map_err(|e| R2psRequestError::ServiceError(e))?;


        let jwe = encrypt_with_ec_pem(&response, &client_metadata.client_public_key)
            .map_err(|_| R2psRequestError::EncryptionError)?;

        let jws = jws_with_jwk(&jwe, service_request.nonce)
            .map_err(|_| R2psRequestError::JwsError)?;

        debug!("JWKS {:?}", jws);

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


#[instrument(skip(r2ps_service, decrypted_payload), level = "debug", err)]
fn authenticate(decrypted_payload: &Vec<u8>, device_id: &str, r2ps_service: &R2psService, pake_session_id: &String) -> Result<Vec<u8>, ServiceRequestError> {
    match PakeRequestPayload::deserialize(&decrypted_payload) {
        Ok(pake_payload) => {
            info!("deserialized pake payload req={}", pake_payload.request_data);
            let rb: Vec<u8> = general_purpose::STANDARD
                .decode(pake_payload.request_data)
                .expect("Failed to decode base64");

            let context = "RPS-Ops".as_bytes();
            let client = device_id.as_bytes();
            let server = "https://cloud-wallet.digg.se/rhsm".as_bytes();

            info!("######## OPAQUE context: '{}' hex: {}", String::from_utf8_lossy(context), hex::encode(context));
            info!("######## OPAQUE client: '{}' hex: {}", String::from_utf8_lossy(client), hex::encode(client));
            info!("######## OPAQUE server: '{}' hex: {}", String::from_utf8_lossy(server), hex::encode(server));

            let server_login_parameters = ServerLoginParameters{
                context: Some(context),
                identifiers: Identifiers {
                    client: Some(client),
                    server: Some(server),
                },
            };

            match pake_payload.state {
                PakeState::Evaluate => {
                    let serialized = r2ps_service.client_repository_spi_port.client_metadata(device_id);
                    let password_file_serialized = serialized.and_then(|meta_data| {meta_data.password_file.clone()}).unwrap();
                    //let password_file_serialized =
                      //  ServerRegistration::<DefaultCipherSuite>::deserialize(&r2ps_service.client_repository_spi_port.client_metadata(device_id).unwrap().password_file.unwrap().to_bytes()).unwrap();
                    let password_file = ServerRegistration::<DefaultCipherSuite>::deserialize(&password_file_serialized).unwrap();
                    info!("password file: {:?}", password_file);
                    let mut server_rng = OsRng;

                    let credential_request = CredentialRequest::deserialize(&rb).unwrap();

                    match ServerLogin::start(
                        &mut server_rng,
                        &r2ps_service.opaque_server_setup,
                        Some(password_file),
                        credential_request,
                        device_id.as_bytes(),
                        server_login_parameters,
                    ) {
                        Ok(server_login_start_result) => {
                            info!("server_login_start_result = {:?}", server_login_start_result);
                            let credential_response_bytes = server_login_start_result.message.serialize();
                            let session = Arc::new(LoginSession::new(server_login_start_result.state));
                            r2ps_service.session_key_spi_port.store_pending_auth(&pake_session_id, &session);
                            let pake_response = PakeResponsePayload {
                                pake_session_id: Some(pake_session_id.to_string()),
                                task: None,
                                response_data: Some(general_purpose::STANDARD.encode(credential_response_bytes.to_vec())),
                                message: None,
                                session_expiration_time: None,
                            };
                            match serde_json::to_vec(&pake_response) {
                                Ok(payload_vec) => Ok(payload_vec),
                                Err(e) => Err(ServiceRequestError::Unknown)
                            }
                        },
                        Err(e) => Err(ServiceRequestError::Unknown)
                    }
                }
                PakeState::Finalize => {
                    let mut session = r2ps_service.session_key_spi_port.get_pending_auth(&pake_session_id).ok_or(ServiceRequestError::Unknown)?;

                    let server_login = session.take().unwrap();
                    let result = server_login.finish(
                        CredentialFinalization::deserialize(&rb).unwrap(),
                        server_login_parameters,
                    )
                    .unwrap();

                    info!("SESSION KEY: {:?}", result.session_key);

                    let msg = br#"{"msg":"OK"}"#.to_vec();
                    let pake_response = PakeResponsePayload {
                        pake_session_id: Some(pake_session_id.to_string()),
                        task: None,
                        response_data: Some(general_purpose::STANDARD.encode(msg.to_vec())),
                        message: None,
                        session_expiration_time: Some(Utc::now().timestamp()),
                    };
                    match serde_json::to_vec(&pake_response) {
                        Ok(payload_vec) => Ok(payload_vec),
                        Err(e) => Err(ServiceRequestError::Unknown)
                    }
                }
            }

        },
        Err(e) => {
            info!("deserialize {:?}", e);
            Err(ServiceRequestError::Unknown)
        }
    }
}

#[instrument(skip(r2ps_service, decrypted_payload), level = "debug", err)]
fn pin_registration(decrypted_payload: &Vec<u8>, device_id: &str, r2ps_service: &R2psService) -> Result<Vec<u8>, ServiceRequestError> {
    match PakeRequestPayload::deserialize(&decrypted_payload) {
        Ok(pake_payload) => {
            info!("deserialized pake payload req={}", pake_payload.request_data);
            let rb: Vec<u8> = general_purpose::STANDARD
                .decode(pake_payload.request_data)
                .expect("Failed to decode base64");


            match pake_payload.state {
                PakeState::Evaluate => {
                    let reg_req: RegistrationRequest<DefaultCipherSuite> = match RegistrationRequest::deserialize(&rb) {
                        Ok(reg_req) => {
                            info!("deserialized registration request: {:?}", reg_req);
                            reg_req
                        },
                        Err(err) => {
                            panic!("error decoding pake request bytes {:?}", err);
                        }
                    };
                    match ServerRegistration::<DefaultCipherSuite>::start(
                        &r2ps_service.opaque_server_setup,
                        reg_req,
                        device_id.as_bytes(),
                    ) {
                        Ok(d) => {
                            info!("START {:?}", d.message);
                            let msg = d.message.serialize().to_vec();
                            let pake_response = PakeResponsePayload {
                                pake_session_id: None,
                                task: None,
                                response_data: Some(general_purpose::STANDARD.encode(msg.to_vec())),
                                message: None,
                                session_expiration_time: None,
                            };

                            match serde_json::to_vec(&pake_response) {
                                Ok(payload_vec) => Ok(payload_vec),
                                Err(e) => Err(ServiceRequestError::Unknown)
                            }
                        },
                        Err(e) => {
                            error!("ERROR {:?}", e);
                            Err(ServiceRequestError::Unknown)
                        }
                    }
                }
            ,
            PakeState::Finalize => {
                let reg_req: RegistrationUpload<DefaultCipherSuite> = match RegistrationUpload::deserialize(&rb) {
                    Ok(reg_req) => {
                        info!("deserialized registration upload: {:?}", reg_req);
                        reg_req
                    },
                    Err(err) => {
                        panic!("error decoding pake registration upload bytes {:?}", err);
                    }
                };

                let password_file = ServerRegistration::<DefaultCipherSuite>::finish(
                    reg_req,
                );

                info!("password file: {:?}", password_file.serialize());

                match r2ps_service.client_repository_spi_port.client_metadata(device_id) {
                    Some(client_metadata) => {
                        let _ = r2ps_service.client_repository_spi_port.store_metadata(ClientMetadata {
                            client_id: client_metadata.client_id,
                            wallet_id: client_metadata.wallet_id,
                            client_public_key: client_metadata.client_public_key,
                            password_file: Some(password_file.serialize()),
                        });
                        info!("Store metadata: {:?}", password_file);

                    },
                    _ => {}
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
                    Err(e) => Err(ServiceRequestError::Unknown)
                }


            }
        }
    },
    Err(e) => {
            info!("deserialize {:?}", e);
            Err(ServiceRequestError::Unknown)
        }
    }
}

#[instrument(skip(r2ps_service, decrypted_payload), level = "debug", err)]
fn process_service_request(service_request: &ServiceRequest, decrypted_payload: &Vec<u8>, device_id: &str, r2ps_service: &R2psService) -> Result<Vec<u8>, ServiceRequestError> {
    let pake_session_id = match &service_request.pake_session_id {
        Some(session_id) => session_id.to_string(),
        None => Uuid::new_v4().to_string(),
    };
    match service_request.service_type {
        ServiceTypeId::Authenticate => authenticate(decrypted_payload, device_id, &r2ps_service, &pake_session_id),
        ServiceTypeId::PinRegistration => pin_registration(decrypted_payload, device_id, &r2ps_service),
        ServiceTypeId::PinChange => Err(ServiceRequestError::Unknown),
        ServiceTypeId::HsmEcdsa => Err(ServiceRequestError::Unknown),
        ServiceTypeId::HsmEcdh => Err(ServiceRequestError::Unknown),
        ServiceTypeId::HsmEcKeygen => Err(ServiceRequestError::Unknown),
        ServiceTypeId::HsmEcDeleteKey => Err(ServiceRequestError::Unknown),
        ServiceTypeId::HsmListKeys => Err(ServiceRequestError::Unknown),
        ServiceTypeId::SessionEnd => Err(ServiceRequestError::Unknown),
        ServiceTypeId::SessionContextEnd => Err(ServiceRequestError::Unknown),
        ServiceTypeId::Store => Err(ServiceRequestError::Unknown),
        ServiceTypeId::Retrieve => Err(ServiceRequestError::Unknown),
        ServiceTypeId::Log => Err(ServiceRequestError::Unknown),
        ServiceTypeId::GetLog => Err(ServiceRequestError::Unknown),
        ServiceTypeId::Info => Err(ServiceRequestError::Unknown),
    }
}
fn decode_r2ps_request_jws(input: &R2psRequest, client_public_key: &Pem) -> Result<ServiceRequest, ServiceRequestError>{
    let pem_string = pem::encode(&client_public_key);

    match DecodingKey::from_ec_pem(pem_string.as_bytes()) {
        Ok(decoding_key) => {
            let mut validation = Validation::new(Algorithm::ES256);
            validation.validate_exp = false;  // Your token doesn't have 'exp'
            validation.required_spec_claims.clear();
            match decode::<ServiceRequest>(&input.payload, &decoding_key, &validation) {
                Ok(service_request_claims) => {
                    info!("decoded claims: {:?}", service_request_claims);
                    Ok(service_request_claims.claims)
                },
                Err(error) => {
                    error!("Error decoding jws claims: {:?}", error);
                    Err(ServiceRequestError::JwsError)
                }
            }
        },
        Err(error) => {
            error!("invalid client public key: {:?}", error);
            Err(ServiceRequestError::InvalidClientPublicKey)
        }
    }
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
                Err(e) =>  {
                    error!("********1 {:?}", e);
                    Err(ServiceRequestError::Unknown)
                },
            },
            Err(e) => {
                error!("********2 {:?}", e);
                Err(ServiceRequestError::Unknown)
            }
        }

}



fn decrypt_service_data_jwe(service_request: &ServiceRequest, server_private_key: &Pem) -> Result<Vec<u8>, ServiceRequestError> {
    match &service_request.service_data {
        Some(service_data) => {
            info!("SERVICE DATA ******* {} ", service_data);
            match BASE64_STANDARD.decode(&service_data) {
                Ok(data) => {
                    match String::from_utf8(data) {
                        Ok(decoded_string) => {
                            let private_key_pem_string = pem::encode(&server_private_key);
                            let parts: Vec<&str> = decoded_string.split('.').collect();
                            match parts.len() {
                                5 => {
                                    match ECDH_ES.decrypter_from_pem(&private_key_pem_string) {
                                        Ok(decrypter) => {
                                            match josekit::jwe::deserialize_compact(&decoded_string, &decrypter) {
                                                Ok((payload, header)) => {
                                                    info!("decrypted JWS payload: {:?}", payload);
                                                    match String::from_utf8(payload.clone()) {
                                                        Ok(decrypted_text) => println!("================>Decrypted text: {}", decrypted_text),
                                                        Err(error) => println!("Error decrypting JWS payload: {:?}", error),
                                                    };
                                                    Ok(payload.to_vec())
                                                },
                                                Err(error) => Err(ServiceRequestError::Unknown)
                                            }

                                        },
                                        Err(error) => Err(ServiceRequestError::Unknown)
                                    }

                                },
                                _ => Err(ServiceRequestError::JweError)
                            }
                        },
                        Err(error) => Err(ServiceRequestError::Unknown),
                    }
                },
                Err(error) => Err(ServiceRequestError::Unknown),
            }
        }
        None => Err(ServiceRequestError::Unknown),
    }
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


fn jws_with_jwk(data: &str, nonce: Option<String>) ->  Result<String, ServiceRequestError>  {
    let now = Utc::now();    // Get duration in ms since Unix epoch
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