// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use hsm_common::jose as hsmjose;
use josekit::jwe::ECDH_ES;
use josekit::jwe::alg::ecdh_es::EcdhEsJweDecrypter;
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
        let jwk = Jwk::try_from(key).map_err(|e| {
            error!("Failed to convert device JWK: {:?}", e);
            JoseError::InvalidKey
        })?;
        hsmjose::jws_verify(jws, &jwk).map_err(|e| {
            error!("Device JWS verification failed: {:?}", e);
            JoseError::VerifyError
        })
    }

    fn peek_kid(&self, compact: &str) -> Option<String> {
        hsmjose::peek_kid(compact)
    }

    fn jwe_encrypt<'a>(
        &self,
        payload: &[u8],
        key: JweEncryptionKey<'a>,
    ) -> Result<String, JoseError> {
        match key {
            JweEncryptionKey::Session(session_key) => {
                hsmjose::jwe_encrypt_dir(payload, session_key.as_ref(), "session").map_err(|e| {
                    error!("Session JWE encryption failed: {:?}", e);
                    JoseError::EncryptError
                })
            }
            JweEncryptionKey::Device(ec_jwk) => {
                let jwk = Jwk::try_from(ec_jwk).map_err(|e| {
                    error!("Failed to convert device JWK: {:?}", e);
                    JoseError::InvalidKey
                })?;
                hsmjose::jwe_encrypt_ecdh_es(payload, &jwk, "device").map_err(|e| {
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
                let (payload, header) = josekit::jwe::deserialize_compact(jwe_str, &self.decrypter)
                    .map_err(|e| {
                        error!("Device JWE decryption failed: {:?}", e);
                        JoseError::DecryptError
                    })?;
                debug!("Inner JWE header (device): {:#?}", header);
                Ok(payload)
            }
            JweDecryptionKey::Session(session_key) => {
                hsmjose::jwe_decrypt_dir(jwe_str, session_key.as_ref()).map_err(|e| {
                    error!("Session JWE decryption failed: {:?}", e);
                    JoseError::DecryptError
                })
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
