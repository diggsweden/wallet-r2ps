use std::fmt;
use std::marker::PhantomData;

use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use josekit::jwe;
use josekit::jwe::alg::direct::DirectJweAlgorithm;
use josekit::jwe::{ECDH_ES, JweHeader};
use pem::Pem;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use crate::application::session_key_spi_port::SessionKey;
use crate::domain::value_objects::r2ps::InnerRequest;
#[cfg(feature = "openapi")]
use crate::domain::value_objects::r2ps::InnerResponse;
use crate::domain::{EncryptOption, ServiceRequestError};

/// A JWE (JSON Web Encryption) compact serialization string containing an encrypted payload of
/// type `T`. Encrypted with either the session key (AES-256-GCM via "dir" algorithm) or the
/// device's public key (ECDH-ES), depending on the operation type.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct TypedJwe<T>(String, #[serde(skip)] PhantomData<T>);

impl<T> Clone for TypedJwe<T> {
    fn clone(&self) -> Self {
        TypedJwe(self.0.clone(), PhantomData)
    }
}

impl<T> fmt::Debug for TypedJwe<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TypedJwe").field(&self.0).finish()
    }
}

impl<T> TypedJwe<T> {
    pub fn new(jwe: String) -> Self {
        Self(jwe, PhantomData)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    /// Peeks into the JWE header without decrypting and returns the kid (key ID) if present.
    pub fn peek_kid(&self) -> Result<Option<String>, ServiceRequestError> {
        // Split JWE compact serialization (header.encrypted_key.iv.ciphertext.tag)
        let parts: Vec<&str> = self.0.split('.').collect();
        if parts.is_empty() {
            return Err(ServiceRequestError::JweError);
        }

        // Decode the header (first part)
        let header_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|_| ServiceRequestError::JweError)?;

        let header: serde_json::Value =
            serde_json::from_slice(&header_bytes).map_err(|_| ServiceRequestError::JweError)?;

        Ok(header
            .get("kid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    fn decrypt_with_ec_pem(&self, private_key: &Pem) -> Result<Vec<u8>, ServiceRequestError> {
        let decrypter = ECDH_ES.decrypter_from_pem(pem::encode(private_key))?;
        let (payload, header) = jwe::deserialize_compact(&self.0, &decrypter)?;

        debug!("Inner JWE header: {:#?}", header);

        Ok(payload)
    }

    fn decrypt_with_aes(&self, session_key: &SessionKey) -> Result<Vec<u8>, ServiceRequestError> {
        let decrypter = DirectJweAlgorithm::Dir
            .decrypter_from_bytes(session_key.as_ref())
            .map_err(|e| {
                error!("Failed to create decrypter: {:?}", e);
                ServiceRequestError::JweError
            })?;

        let (payload, header) = jwe::deserialize_compact(&self.0, &decrypter).map_err(|e| {
            error!("Failed to decrypt: {:?}", e);
            ServiceRequestError::JweError
        })?;

        debug!("Inner JWE header: {:#?}", header);

        Ok(payload)
    }
}

impl<T: DeserializeOwned> TypedJwe<T> {
    pub fn decrypt(
        &self,
        enc_option: EncryptOption,
        server_private_key: &Pem,
        session_key: Option<&SessionKey>,
    ) -> Result<T, ServiceRequestError> {
        let payload = match enc_option {
            EncryptOption::Session => {
                let key = session_key.ok_or(ServiceRequestError::UnknownSession)?;
                self.decrypt_with_aes(key)?
            }
            EncryptOption::Device => self.decrypt_with_ec_pem(server_private_key)?,
        };

        serde_json::from_slice(&payload).map_err(|e| {
            error!("Failed to deserialize inner JWE payload: {:?}", e);
            ServiceRequestError::JweError
        })
    }
}

impl<T: Serialize> TypedJwe<T> {
    pub fn encrypt(value: &T, session_key: &SessionKey) -> Result<Self, ServiceRequestError> {
        let payload = serde_json::to_vec(value).map_err(|e| {
            error!("Failed to serialize JWE payload: {:?}", e);
            ServiceRequestError::Unknown
        })?;

        let mut header = JweHeader::new();
        header.set_algorithm("dir");
        header.set_content_encryption("A256GCM");
        header.set_key_id("session");

        let encrypter = DirectJweAlgorithm::Dir
            .encrypter_from_bytes(session_key.as_ref())
            .map_err(|e| {
                error!("Failed to create encrypter: {:?}", e);
                ServiceRequestError::Unknown
            })?;

        let jwe = jwe::serialize_compact(&payload, &header, &encrypter).map_err(|e| {
            error!("Failed to encrypt: {:?}", e);
            ServiceRequestError::Unknown
        })?;

        Ok(Self(jwe, PhantomData))
    }

    pub fn encrypt_with_jwk(
        value: &T,
        client_public_key: &josekit::jwk::Jwk,
    ) -> Result<Self, ServiceRequestError> {
        let payload = serde_json::to_vec(value).map_err(|e| {
            error!("Failed to serialize JWE payload: {:?}", e);
            ServiceRequestError::JweError
        })?;

        let mut header = JweHeader::new();
        header.set_algorithm("ECDH-ES");
        header.set_content_encryption("A256GCM");
        header.set_key_id("device");

        match ECDH_ES.encrypter_from_jwk(client_public_key) {
            Ok(encrypter) => match jwe::serialize_compact(&payload, &header, &encrypter) {
                Ok(payload_bytes) => Ok(Self(payload_bytes, PhantomData)),
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
}

// ---------------------------------------------------------------------------
// ToSchema implementations for concrete TypedJwe<T> instantiations.
//
// Mirrors the pattern used for TypedJws<T>: name() returns "TypedJwe" so that
// utoipa's auto-generated component key "TypedJwe_{TypeArg}" matches the key
// pushed by schemas(), producing a single component per variant.
// ---------------------------------------------------------------------------

#[cfg(feature = "openapi")]
/// Build the OpenAPI schema for a concrete `TypedJwe<$inner>` instantiation.
/// See the `jws_schema!` macro in typed_jws.rs for the rationale behind the
/// `Extensions`/`DerefMut` trick used to emit the non-`x-`-prefixed
/// `contentSchema` key.
macro_rules! inner_jwe_schema {
    ($inner:ty, $description:literal) => {{
        let mut exts = utoipa::openapi::extensions::Extensions::default();
        exts.insert(
            "contentSchema".to_string(),
            serde_json::json!({
                "$ref": concat!("#/components/schemas/", stringify!($inner))
            }),
        );

        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::SchemaType::new(
                utoipa::openapi::schema::Type::String,
            ))
            .content_media_type("application/json")
            .description(Some($description))
            .extensions(Some(exts))
            .build()
            .into()
    }};
}

#[cfg(feature = "openapi")]
macro_rules! impl_inner_jwe_to_schema {
    ($inner:ty, $description:literal) => {
        impl utoipa::__dev::ComposeSchema for TypedJwe<$inner> {
            fn compose(
                _generics: Vec<utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>>,
            ) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                inner_jwe_schema!($inner, $description)
            }
        }

        impl utoipa::ToSchema for TypedJwe<$inner> {
            fn name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed("TypedJwe")
            }

            fn schemas(
                schemas: &mut Vec<(
                    String,
                    utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
                )>,
            ) {
                schemas.push((
                    concat!("TypedJwe_", stringify!($inner)).to_string(),
                    inner_jwe_schema!($inner, $description),
                ));
            }
        }
    };
}

#[cfg(feature = "openapi")]
impl_inner_jwe_to_schema!(
    InnerRequest,
    "JWE compact serialization (RFC 7516). The encrypted payload is a JSON-encoded InnerRequest."
);

#[cfg(feature = "openapi")]
impl_inner_jwe_to_schema!(
    InnerResponse,
    "JWE compact serialization (RFC 7516). The encrypted payload is a JSON-encoded InnerResponse."
);

impl TypedJwe<InnerRequest> {
    /// High-level decryption that peeks the kid and validates the encryption option against the
    /// request type.
    pub fn decrypt_request(
        &self,
        server_private_key: &Pem,
        session_key: Option<&SessionKey>,
    ) -> Result<InnerRequest, ServiceRequestError> {
        let peeked_kid = self.peek_kid().map_err(|_| ServiceRequestError::JweError)?;
        debug!("Peeked inner JWE kid: {:?}", peeked_kid);

        let enc_option = match peeked_kid.as_deref() {
            Some("session") => EncryptOption::Session,
            Some("device") => EncryptOption::Device,
            _ => {
                error!("Unknown encryption option in JWE kid: {:?}", peeked_kid);
                return Err(ServiceRequestError::JweError);
            }
        };

        debug!("Decrypting inner request using {:?} encryption", enc_option);

        let inner_request = self
            .decrypt(enc_option, server_private_key, session_key)
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
