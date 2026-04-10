// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::port::outgoing::jose_port;
use crate::application::port::outgoing::session_state_spi_port::SessionKey;
use crate::application::{OuterError, UpstreamError};
use crate::domain::{
    EcPublicJwk, EncryptOption, InnerRequest, InnerResponse, OuterRequest, OuterResponse, TypedJwe,
    TypedJws,
};
use tracing::{debug, error};

pub trait OuterRequestExt {
    fn from_jws(
        jws: &str,
        jose: &dyn jose_port::JosePort,
        key: &EcPublicJwk,
    ) -> Result<OuterRequest, UpstreamError>;

    fn decrypt_inner(
        &self,
        jose: &dyn jose_port::JosePort,
        session_key: Option<&SessionKey>,
    ) -> Result<InnerRequest, OuterError>;
}

impl OuterRequestExt for OuterRequest {
    fn from_jws(
        jws: &str,
        jose: &dyn jose_port::JosePort,
        key: &EcPublicJwk,
    ) -> Result<OuterRequest, UpstreamError> {
        let bytes = jose
            .jws_verify_device(jws, key)
            .map_err(|_| UpstreamError::OuterJwsInvalid)?;

        serde_json::from_slice(&bytes).map_err(|e| {
            error!("Failed to deserialize outer request: {:?}", e);
            UpstreamError::OuterJwsInvalid
        })
    }

    fn decrypt_inner(
        &self,
        jose: &dyn jose_port::JosePort,
        session_key: Option<&SessionKey>,
    ) -> Result<InnerRequest, OuterError> {
        let jwe = self.inner_jwe.as_ref().ok_or(OuterError::InnerJweMissing)?;

        let peeked_kid = jose.peek_kid(jwe.as_str());
        debug!("Peeked inner JWE kid: {:?}", peeked_kid);

        let (bytes, enc_option) = match peeked_kid.as_deref() {
            Some("session") => {
                let key = session_key.ok_or(OuterError::SessionKeyMissing)?;
                let bytes = jose
                    .jwe_decrypt(jwe.as_str(), jose_port::JweDecryptionKey::Session(key))
                    .map_err(|_| OuterError::InnerJweDecryptFailed)?;
                (bytes, EncryptOption::Session)
            }
            Some("device") => {
                let bytes = jose
                    .jwe_decrypt(jwe.as_str(), jose_port::JweDecryptionKey::Device)
                    .map_err(|_| OuterError::InnerJweDecryptFailed)?;
                (bytes, EncryptOption::Device)
            }
            _ => {
                error!("Unknown encryption option in JWE kid: {:?}", peeked_kid);
                return Err(OuterError::UnknownEncryptionOption);
            }
        };

        let inner_request: InnerRequest =
            serde_json::from_slice(&bytes).map_err(|_| OuterError::InnerJweDecryptFailed)?;

        if inner_request.request_type.encrypt_option() != enc_option {
            error!(
                "Encryption option mismatch for {:?}: expected {:?}, got {:?}",
                inner_request.request_type,
                inner_request.request_type.encrypt_option(),
                enc_option
            );
            return Err(OuterError::InnerJweDecryptFailed);
        }

        Ok(inner_request)
    }
}

pub trait OuterResponseExt {
    fn sign(
        &self,
        jose: &dyn jose_port::JosePort,
    ) -> Result<TypedJws<OuterResponse>, UpstreamError>;
}

impl OuterResponseExt for OuterResponse {
    fn sign(
        &self,
        jose: &dyn jose_port::JosePort,
    ) -> Result<TypedJws<OuterResponse>, UpstreamError> {
        let bytes = serde_json::to_vec(self).map_err(|e| {
            error!("Failed to serialize outer response: {:?}", e);
            UpstreamError::EncodeFailed("outer_response_sign_failed")
        })?;

        let jws_str = jose
            .jws_sign(&bytes)
            .map_err(|_| UpstreamError::EncodeFailed("outer_response_sign_failed"))?;

        Ok(TypedJws::new(jws_str))
    }
}

pub trait InnerResponseExt {
    fn encrypt(
        &self,
        jose: &dyn jose_port::JosePort,
        key: jose_port::JweEncryptionKey<'_>,
    ) -> Result<TypedJwe<InnerResponse>, UpstreamError>;
}

impl InnerResponseExt for InnerResponse {
    fn encrypt(
        &self,
        jose: &dyn jose_port::JosePort,
        key: jose_port::JweEncryptionKey<'_>,
    ) -> Result<TypedJwe<InnerResponse>, UpstreamError> {
        let bytes = serde_json::to_vec(self).map_err(|e| {
            error!("Failed to serialize inner response: {:?}", e);
            UpstreamError::EncodeFailed("inner_response_encrypt_failed")
        })?;

        let jwe_str = jose
            .jwe_encrypt(&bytes, key)
            .map_err(|_| UpstreamError::EncodeFailed("inner_response_encrypt_failed"))?;

        Ok(TypedJwe::new(jwe_str))
    }
}
