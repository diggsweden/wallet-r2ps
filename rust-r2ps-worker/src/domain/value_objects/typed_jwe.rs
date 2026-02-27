use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use crate::domain::value_objects::r2ps::InnerRequest;
#[cfg(feature = "openapi")]
use crate::domain::value_objects::r2ps::InnerResponse;

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
