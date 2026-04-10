// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Typed JWS/JWE compact-serialization wrappers.
//!
//! `TypedJws<T>` and `TypedJwe<T>` are transparent `String` newtypes whose
//! phantom type parameter documents what is signed/encrypted inside, giving
//! compile-time protection against mixing up token fields.

use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// A JWS compact serialization string whose payload is a signed `T`.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct TypedJws<T>(String, #[serde(skip)] PhantomData<T>);

impl<T> TypedJws<T> {
    pub fn new(jws: String) -> Self {
        Self(jws, PhantomData)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl<T> Clone for TypedJws<T> {
    fn clone(&self) -> Self {
        TypedJws(self.0.clone(), PhantomData)
    }
}

impl<T> fmt::Debug for TypedJws<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TypedJws").field(&self.0).finish()
    }
}

/// A JWE compact serialization string whose plaintext is an encrypted `T`.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct TypedJwe<T>(String, #[serde(skip)] PhantomData<T>);

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

// ─── OpenAPI schemas for concrete instantiations available in this crate ─────

#[cfg(feature = "openapi")]
use crate::{InnerRequest, InnerResponse, OuterRequest, OuterResponse};

#[cfg(feature = "openapi")]
macro_rules! jwe_schema {
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
macro_rules! impl_jwe_schema {
    ($inner:ty, $description:literal) => {
        impl utoipa::__dev::ComposeSchema for TypedJwe<$inner> {
            fn compose(
                _generics: Vec<utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>>,
            ) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                jwe_schema!($inner, $description)
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
                    jwe_schema!($inner, $description),
                ));
            }
        }
    };
}

#[cfg(feature = "openapi")]
macro_rules! jws_schema {
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
macro_rules! impl_jws_schema {
    ($inner:ty, $description:literal) => {
        impl utoipa::__dev::ComposeSchema for TypedJws<$inner> {
            fn compose(
                _generics: Vec<utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>>,
            ) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                jws_schema!($inner, $description)
            }
        }
        impl utoipa::ToSchema for TypedJws<$inner> {
            fn name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed("TypedJws")
            }
            fn schemas(
                schemas: &mut Vec<(
                    String,
                    utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
                )>,
            ) {
                schemas.push((
                    concat!("TypedJws_", stringify!($inner)).to_string(),
                    jws_schema!($inner, $description),
                ));
            }
        }
    };
}

#[cfg(feature = "openapi")]
impl_jwe_schema!(
    InnerRequest,
    "JWE compact serialization (RFC 7516). Encrypted payload is a JSON-encoded InnerRequest."
);

#[cfg(feature = "openapi")]
impl_jwe_schema!(
    InnerResponse,
    "JWE compact serialization (RFC 7516). Encrypted payload is a JSON-encoded InnerResponse."
);

#[cfg(feature = "openapi")]
impl_jws_schema!(
    OuterRequest,
    "JWS compact serialization (RFC 7515). Signed payload is a JSON-encoded OuterRequest."
);

#[cfg(feature = "openapi")]
impl_jws_schema!(
    OuterResponse,
    "JWS compact serialization (RFC 7515). Signed payload is a JSON-encoded OuterResponse."
);
