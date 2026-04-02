use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use josekit::jwe::alg::direct::DirectJweAlgorithm;
use josekit::jwe::alg::ecdh_es::EcdhEsJweDecrypter;
use josekit::jwe::{self, ECDH_ES, JweHeader};
use josekit::jwk::Jwk;
use josekit::jws::ES256;
use josekit::jws::alg::ecdsa::{EcdsaJwsSigner, EcdsaJwsVerifier};
use josekit::jwt;
use p256::SecretKey;
use p256::pkcs8::EncodePrivateKey;
use p256::pkcs8::EncodePublicKey;
use tracing::{debug, error};

use crate::application::port::outgoing::jose_port::{
    JoseError, JosePort, JweDecryptionKey, JweEncryptionKey,
};
use crate::domain::EcPublicJwk;
use crate::infrastructure::config::jose_utils;

pub struct JoseAdapter {
    signer: EcdsaJwsSigner,
    verifier: EcdsaJwsVerifier,
    decrypter: EcdhEsJweDecrypter,
    public_key: EcPublicJwk,
    kid: String,
}

impl JoseAdapter {
    pub fn new(secret_key: SecretKey) -> Result<Self, JoseError> {
        let private_pem = secret_key.to_pkcs8_pem(Default::default()).map_err(|e| {
            error!("Failed to encode private key as PKCS8 PEM: {:?}", e);
            JoseError::InvalidKey
        })?;

        let signer = ES256.signer_from_pem(private_pem.as_bytes()).map_err(|e| {
            error!("Failed to create JWS signer: {:?}", e);
            JoseError::InvalidKey
        })?;

        let public_pem = secret_key
            .public_key()
            .to_public_key_pem(Default::default())
            .map_err(|e| {
                error!("Failed to encode public key as SPKI PEM: {:?}", e);
                JoseError::InvalidKey
            })?;

        let verifier = ES256
            .verifier_from_pem(public_pem.as_bytes())
            .map_err(|e| {
                error!("Failed to create JWS verifier: {:?}", e);
                JoseError::InvalidKey
            })?;

        let decrypter = ECDH_ES
            .decrypter_from_pem(private_pem.as_bytes())
            .map_err(|e| {
                error!("Failed to create JWE decrypter: {:?}", e);
                JoseError::InvalidKey
            })?;

        let public_key = jose_utils::ec_public_key_from_secret(&secret_key);
        let kid = public_key.kid.clone();

        Ok(Self {
            signer,
            verifier,
            decrypter,
            public_key,
            kid,
        })
    }
}

fn ec_public_jwk_to_jwk(ec_jwk: &EcPublicJwk) -> Result<Jwk, JoseError> {
    let mut jwk = Jwk::new("EC");
    jwk.set_curve(&ec_jwk.crv);
    jwk.set_parameter("x", Some(serde_json::Value::String(ec_jwk.x.clone())))
        .map_err(|_| JoseError::InvalidKey)?;
    jwk.set_parameter("y", Some(serde_json::Value::String(ec_jwk.y.clone())))
        .map_err(|_| JoseError::InvalidKey)?;
    jwk.set_key_id(&ec_jwk.kid);
    Ok(jwk)
}

impl JosePort for JoseAdapter {
    fn jws_sign(&self, payload_json: &[u8]) -> Result<String, JoseError> {
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(payload_json)
            .map_err(|e| {
                error!("Failed to parse payload JSON: {:?}", e);
                JoseError::SignError
            })?;
        let payload = josekit::jwt::JwtPayload::from_map(map).map_err(|e| {
            error!("Failed to create JwtPayload: {:?}", e);
            JoseError::SignError
        })?;
        let mut header = josekit::jws::JwsHeader::new();
        header.set_key_id(&self.kid);
        jwt::encode_with_signer(&payload, &header, &self.signer).map_err(|e| {
            error!("Failed to encode JWS: {:?}", e);
            JoseError::SignError
        })
    }

    fn jws_verify_server(&self, jws: &str) -> Result<Vec<u8>, JoseError> {
        let (payload, _header) = jwt::decode_with_verifier(jws, &self.verifier).map_err(|e| {
            error!("Server JWS verification failed: {:?}", e);
            JoseError::VerifyError
        })?;
        debug!("Decoded server JWS payload");
        Ok(payload.to_string().into_bytes())
    }

    fn jws_verify_device(&self, jws: &str, key: &EcPublicJwk) -> Result<Vec<u8>, JoseError> {
        let jwk = ec_public_jwk_to_jwk(key)?;
        let verifier = ES256.verifier_from_jwk(&jwk).map_err(|e| {
            error!("Failed to create verifier from device JWK: {:?}", e);
            JoseError::InvalidKey
        })?;
        let (payload, _header) = jwt::decode_with_verifier(jws, &verifier).map_err(|e| {
            error!("Device JWS verification failed: {:?}", e);
            JoseError::VerifyError
        })?;
        debug!("Decoded device JWS payload");
        Ok(payload.to_string().into_bytes())
    }

    fn peek_kid(&self, compact: &str) -> Result<Option<String>, JoseError> {
        let header_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(compact.split('.').next().unwrap_or(""))
            .map_err(|_| JoseError::VerifyError)?;
        let header: serde_json::Value =
            serde_json::from_slice(&header_bytes).map_err(|_| JoseError::VerifyError)?;
        Ok(header
            .get("kid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    fn jwe_encrypt<'a>(
        &self,
        payload: &[u8],
        key: JweEncryptionKey<'a>,
    ) -> Result<String, JoseError> {
        match key {
            JweEncryptionKey::Session(session_key) => {
                let mut header = JweHeader::new();
                header.set_algorithm("dir");
                header.set_content_encryption("A256GCM");
                header.set_key_id("session");
                let encrypter = DirectJweAlgorithm::Dir
                    .encrypter_from_bytes(session_key.as_ref())
                    .map_err(|e| {
                        error!("Failed to create session encrypter: {:?}", e);
                        JoseError::EncryptError
                    })?;
                jwe::serialize_compact(payload, &header, &encrypter).map_err(|e| {
                    error!("Session JWE encryption failed: {:?}", e);
                    JoseError::EncryptError
                })
            }
            JweEncryptionKey::Device(ec_jwk) => {
                let jwk = ec_public_jwk_to_jwk(ec_jwk)?;
                let mut header = JweHeader::new();
                header.set_algorithm("ECDH-ES");
                header.set_content_encryption("A256GCM");
                header.set_key_id("device");
                let encrypter = ECDH_ES.encrypter_from_jwk(&jwk).map_err(|e| {
                    error!("Failed to create device encrypter: {:?}", e);
                    JoseError::EncryptError
                })?;
                jwe::serialize_compact(payload, &header, &encrypter).map_err(|e| {
                    error!("Device JWE encryption failed: {:?}", e);
                    JoseError::EncryptError
                })
            }
        }
    }

    fn jwe_decrypt<'a>(
        &self,
        jwe_str: &str,
        key: JweDecryptionKey<'a>,
    ) -> Result<Vec<u8>, JoseError> {
        match key {
            JweDecryptionKey::Device => {
                let (payload, header) = jwe::deserialize_compact(jwe_str, &self.decrypter)
                    .map_err(|e| {
                        error!("Device JWE decryption failed: {:?}", e);
                        JoseError::DecryptError
                    })?;
                debug!("Inner JWE header (device): {:#?}", header);
                Ok(payload)
            }
            JweDecryptionKey::Session(session_key) => {
                let decrypter = DirectJweAlgorithm::Dir
                    .decrypter_from_bytes(session_key.as_ref())
                    .map_err(|e| {
                        error!("Failed to create session decrypter: {:?}", e);
                        JoseError::DecryptError
                    })?;
                let (payload, header) =
                    jwe::deserialize_compact(jwe_str, &decrypter).map_err(|e| {
                        error!("Session JWE decryption failed: {:?}", e);
                        JoseError::DecryptError
                    })?;
                debug!("Inner JWE header (session): {:#?}", header);
                Ok(payload)
            }
        }
    }

    fn jws_public_key(&self) -> &EcPublicJwk {
        &self.public_key
    }

    fn jws_kid(&self) -> &str {
        &self.kid
    }
}
