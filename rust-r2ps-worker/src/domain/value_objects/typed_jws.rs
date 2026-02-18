use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

/// A JWS (JSON Web Signature) compact serialization string containing a signed payload of type
/// `T`. The phantom type parameter encodes what is signed inside, preventing the caller from
/// accidentally passing a `TypedJws<DeviceHsmState>` where a `TypedJws<OuterResponse>` is expected.
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

// ---------------------------------------------------------------------------
// ToSchema implementations for concrete TypedJws<T> instantiations.
//
// utoipa v5 names a generic component as `{name()}_{ConcreteTypeArg}`.  By
// returning just `"TypedJws"` from `name()`, the auto-generated component key
// becomes `"TypedJws_DeviceHsmState"` etc., which matches the key pushed by our
// own `schemas()` impl — giving exactly one component per variant with no
// duplicate.  Fields in containing structs automatically `$ref` that component,
// preserving the type information that would otherwise be lost when a blanket
// `#[schema(value_type = String)]` override is used.
// ---------------------------------------------------------------------------

#[cfg(feature = "openapi")]
use crate::domain::value_objects::client_metadata::DeviceHsmState;
#[cfg(feature = "openapi")]
use crate::domain::value_objects::r2ps::{OuterRequest, OuterResponse};

#[cfg(feature = "openapi")]
/// Build the OpenAPI schema for a concrete `TypedJws<$inner>` instantiation.
///
/// Uses `Extensions` with a direct `HashMap::insert` (bypassing `ExtensionsBuilder`)
/// to inject the non-`x-`-prefixed `contentSchema` key.  `Extensions` derives
/// `Serialize` with `#[serde(flatten)]` on its inner `HashMap`, so every entry is
/// emitted as-is — no prefix is added during serialisation.  The `x-` restriction
/// only applies to the `Deserialize` impl and the `ExtensionsBuilder::add` helper.
macro_rules! jws_schema {
    ($inner:ty, $description:literal) => {{
        let mut exts = utoipa::openapi::extensions::Extensions::default();
        // Insert contentSchema directly into the backing HashMap via DerefMut,
        // bypassing ExtensionsBuilder's automatic x- prefix.
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
macro_rules! impl_jws_to_schema {
    ($inner:ty, $description:literal) => {
        // utoipa v5: PartialSchema is a blanket impl over ComposeSchema.
        // compose() returns the inline string schema; utoipa uses this as the
        // component definition for "TypedJws_{TypeArg}" and generates a $ref to it
        // for every struct field typed as TypedJws<$inner>.
        impl utoipa::__dev::ComposeSchema for TypedJws<$inner> {
            fn compose(
                _generics: Vec<utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>>,
            ) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                jws_schema!($inner, $description)
            }
        }

        impl utoipa::ToSchema for TypedJws<$inner> {
            // Return the bare struct name. utoipa appends "_<ConcreteTypeArg>"
            // to form the final component key, e.g. "TypedJws_DeviceHsmState".
            // Returning just "TypedJws" keeps that key in sync with what schemas()
            // registers below, eliminating the duplicate.
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
impl_jws_to_schema!(
    DeviceHsmState,
    "JWS compact serialization (RFC 7515). The signed payload is a JSON-encoded DeviceHsmState."
);

#[cfg(feature = "openapi")]
impl_jws_to_schema!(
    OuterResponse,
    "JWS compact serialization (RFC 7515). The signed payload is a JSON-encoded OuterResponse."
);

#[cfg(feature = "openapi")]
impl_jws_to_schema!(
    OuterRequest,
    "JWS compact serialization (RFC 7515). The signed payload is a JSON-encoded OuterRequest."
);
