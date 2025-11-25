use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use base64::Engine;
use base64::engine::general_purpose;
use base64::prelude::BASE64_STANDARD;
use josekit::jwe::{JweHeader, ECDH_ES};
use josekit::jwe::enc::{A128CBC_HS256, A128GCM, A192CBC_HS384, A192GCM, A256CBC_HS512, A256GCM};
use josekit::jwk::Jwk;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use jws::compact::decode_unverified;
use opaque_ke::{RegistrationRequest, ServerRegistration, ServerSetup};
use tracing::{debug, error, info, instrument};
use R2psRequestError::ConnectionError;
use crate::application::{R2psRequestError, R2psRequestId, R2psRequestUseCase, R2psResponseSpiPort};
use crate::{DefaultCipherSuite};
use crate::domain::{R2PsResponse, R2psRequest};
use crate::domain::value_objects::r2ps::{Claims, PakeRequestPayload, PakeResponsePayload, ServiceRequest};

#[derive(Clone)]
pub struct R2psService {
    r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
    server_setup: ServerSetup<DefaultCipherSuite>,
}

impl R2psService {
    pub fn new(
        r2ps_response_spi_port: Arc<dyn R2psResponseSpiPort + Send + Sync>,
        server_setup: ServerSetup<DefaultCipherSuite>,
    ) -> Self {
        Self {
            r2ps_response_spi_port,
            server_setup,
        }
    }
}

impl R2psRequestUseCase for R2psService {

    fn execute(&self, r2ps_request: R2psRequest) -> Result<R2psRequestId, R2psRequestError> {
        match process_message(&self.server_setup, &r2ps_request) {
            Ok(r2ps_response) => {
                let request_id = r2ps_request.request_id.clone();
                match self.r2ps_response_spi_port.send(r2ps_response) {
                    Ok(_) => Ok(request_id),
                    Err(_err) => Err(R2psRequestError::ConnectionError), // TODO map error
                }
            },
            Err(error) => {
                error!("error processing message {:?}", error);
                Err(error)
            }
        }

    }
}

#[instrument(name = "worker", skip_all)]
fn process_message( server_setup: &ServerSetup<DefaultCipherSuite>, input: &R2psRequest) -> Result<R2PsResponse, R2psRequestError>  {
    // Transform the message (example: convert to uppercase and add prefix)
    debug!("Received message: {:?}", input);

    // Decode and verify the message.
    let process_result = match decode_unverified(input.payload.as_bytes()) {
        Ok((message, signature)) => {
            debug!("Decoded JWS");

            let proc_resp : Option<R2PsResponse>  =  match serde_json::from_slice::<ServiceRequest>(&message.payload.to_vec()) {
                Ok(msg) => {
                    info!("deserialized jws payload message: {:?}", msg);

                    match msg.context == "hsm" {
                        true => match &msg.pake_session_id {
                            Some(session_id) => {
                                info!("pake_session_id: {:?}", session_id);
                                None
                            },
                            None => match msg.service_type.as_str() {
                                "pin_registration" => {
                                    let private_key_jwk = r#"{
                                      "kty": "EC",
                                      "crv": "P-256",
                                      "x": "pzQxuXLeiPyzitMKQbUSVOD3Axb-l9LqVjs5GnYanA0",
                                      "y": "ZOAJjFE6FqSE8OV1zOPDT4W4DIaDNBVKeDkNus_7wkE",
                                      "d": "_NIIdRGO-qU2bjxTtnZuC45gAg6wZ0UGe9nCeM7wc0w"
                                    }"#;
                                    let service_data = &msg.service_data.unwrap();
                                    info!("SERVICE DATA ******* {} ", service_data);

                                    let decoded_bytes = BASE64_STANDARD.decode(&service_data).unwrap();
                                    let decoded_string = String::from_utf8(decoded_bytes).unwrap();
                                    info!("DECODED SERVICE DATA ******* {} ", decoded_string);

                                    let data = decrypt_jwe_with_ecdh(&decoded_string, private_key_jwk).unwrap();
                                    info!("device decrypted JWE payload: {:?}", data);

                                    match PakeRequestPayload::deserialize(&data) {
                                        Ok(pake_payload) => {
                                            info!("deserialized pake payload req={}", pake_payload.request_data);
                                            let rb: Vec<u8> = general_purpose::STANDARD
                                                .decode(pake_payload.request_data)
                                                .expect("Failed to decode base64");
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
                                                server_setup,
                                                reg_req,
                                                msg.client_id.as_bytes()
                                            ) {
                                                Ok(d) => {
                                                    info!("START {:?}", d.message);
                                                    let msg = d.message.serialize().to_vec();
                                                    let pake_response = PakeResponsePayload{
                                                        pake_session_id: None,
                                                        task: None,
                                                        response_data: Some(general_purpose::STANDARD.encode(msg.to_vec())),
                                                        message: None,
                                                        session_expiration_time: None,
                                                    };

                                                    let jwk: Jwk = serde_json::from_str(private_key_jwk).unwrap();

                                                    let jwe = encrypt_with_ec_jwk(&pake_response, &jwk).unwrap();

                                                    let jws: String = jws_with_jwk(&jwe);
                                                    let res = R2PsResponse {
                                                        request_id: input.request_id.to_string(),
                                                        wallet_id: input.wallet_id.to_string(),
                                                        device_id: input.device_id.to_string(),
                                                        status: 200,
                                                        payload: jws,
                                                    };
                                                    info!("RESPONSE={:?}", res);
                                                    Some(res)
                                                },
                                                Err(e) => {
                                                    error!("ERROR {:?}", e);
                                                    None
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            info!("deserialize {:?}", e);
                                            None
                                        }
                                    }
                                },
                                _ => None
                            } // opaque
                        } ,
                        false => None
                    }
                },
                Err(e) => {
                    error!("Failed to deserialize JSON: {:?}", e);
                    error!("Payload: {:?}", String::from_utf8_lossy(&message.payload));
                    None
                }
            };
            proc_resp
        },
        Err(e) => {
            error!("Failed to decode unverified message: {:?}", e);
            None
        },
    };
    match process_result {
        Some(res) => {
            Ok(res)
        },
        None => {

                Err(ConnectionError)

        }
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


fn decrypt_jwe_with_ecdh(jwe_token: &str, private_key_jwk: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {

    // Parse the private key from JWK format
    let jwk: Jwk = serde_json::from_str(private_key_jwk)?;

    // Parse the JWE to get header - JWE format is: header.encrypted_key.iv.ciphertext.tag
    let parts: Vec<&str> = jwe_token.split('.').collect();
    if parts.len() != 5 {
        return Err("Invalid JWE format".into());
    }

    // Decode the header
    let header_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&parts[0])?;
    //let header = JweHeader::from_bytes(&header_bytes)?;

    // Create a decrypter for ECDH-ES
    let decrypter = ECDH_ES.decrypter_from_jwk(&jwk)?;

    info!("jwk keyid: {:?} decrypter key id: {:?}", &jwk.key_id(), &decrypter.key_id());
    let (payload, header) = josekit::jwe::deserialize_compact(jwe_token, &decrypter)?;
    // Get the content encryption algorithm from the header
    let enc_algorithm = header.content_encryption()
        .ok_or("No encryption algorithm incoming header")?;

    // Get the appropriate content encryption
    let content_encryption = match enc_algorithm {
        "A128GCM" => &A128GCM as &dyn josekit::jwe::JweContentEncryption,
        "A192GCM" => &A192GCM as &dyn josekit::jwe::JweContentEncryption,
        "A256GCM" => &A256GCM as &dyn josekit::jwe::JweContentEncryption,
        "A128CBC-HS256" => &A128CBC_HS256 as &dyn josekit::jwe::JweContentEncryption,
        "A192CBC-HS384" => &A192CBC_HS384 as &dyn josekit::jwe::JweContentEncryption,
        "A256CBC-HS512" => &A256CBC_HS512 as &dyn josekit::jwe::JweContentEncryption,
        _ => return Err(format!("Unsupported encryption algorithm: {}", enc_algorithm).into()),
    };
    let decrypted_text = String::from_utf8(payload.clone())?;
    println!("Decrypted payload: {}", decrypted_text);
    // Decrypt the JWE token - need to pass as bytes
    //let payload = decrypter.decrypt(None, content_encryption, &header)?;

    Ok(payload.to_vec())
}

fn jws_with_jwk(data: &str) -> String {
    let claims = Claims {
        ver: "1.0".to_string(),
        nonce: "7c4baffd469e9285afc867242c3f569f944d56ff8684a7bc34b2b98a5312999b".to_string(),
        iat: 1763997596,
        enc: "device".to_string(),
        data: data.to_string(),
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
    token
}