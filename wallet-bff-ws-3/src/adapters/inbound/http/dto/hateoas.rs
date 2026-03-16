use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;
use utoipa::openapi::schema::{AdditionalProperties, ObjectBuilder};
use utoipa::openapi::{RefOr, Schema};

/// HATEOAS link following REST Level 3 principles (HYP.11-HYP.17).
/// Complies with Swedish REST API Profile v1.2.0.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "href": "https://api.digg.se/r2ps-api/v1/requests/d290f1ee-6c54-4b01-90e6-d701748f0851",
    "rel": "self",
    "method": "GET"
}))]
pub struct Link {
    /// URI of the related resource (MUST be absolute URL - HYP.07, HYP.12)
    pub href: String,

    /// Relation type (e.g., "self", "next", "poll", "cancel") - MUST be present (HYP.15)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel: Option<String>,

    /// HTTP method for the link - MUST be present even for GET (HYP.17)
    pub method: String,

    /// Media type hint (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub type_hint: Option<String>,
}

impl Link {
    /// Create a new link with href and method (both required by Swedish REST API Profile).
    pub fn new(href: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            rel: None,
            method: method.into(),
            type_hint: None,
        }
    }

    pub fn with_rel(mut self, rel: impl Into<String>) -> Self {
        self.rel = Some(rel.into());
        self
    }

    pub fn with_type(mut self, type_hint: impl Into<String>) -> Self {
        self.type_hint = Some(type_hint.into());
        self
    }
}

/// HATEOAS links collection with explicit `self` link and optional additional links.
///
/// The `self` link is mandatory per REST Level 3 / HATEOAS (HYP.11).
/// Additional links (e.g., `poll`, `cancel`, `submit-request`) are serialized
/// alongside `self` via serde flatten.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    /// Self link — MUST be present on every HATEOAS response (HYP.11)
    #[serde(rename = "self")]
    pub self_link: Link,

    /// Additional HATEOAS links (e.g., poll, cancel, submit-request)
    #[serde(flatten)]
    pub additional: HashMap<String, Link>,
}

impl<'s> ToSchema<'s> for Links {
    fn schema() -> (&'s str, RefOr<Schema>) {
        let schema = ObjectBuilder::new()
            .description(Some(
                "HATEOAS links collection. The `self` link is mandatory per REST Level 3 (HYP.11). \
                 Additional links (e.g., poll, cancel) may be present.",
            ))
            .property(
                "self",
                RefOr::Ref(utoipa::openapi::Ref::from_schema_name("Link")),
            )
            .required("self")
            .additional_properties(Some(AdditionalProperties::RefOr(
                RefOr::Ref(utoipa::openapi::Ref::from_schema_name("Link")),
            )))
            .example(Some(serde_json::json!({
                "self": {
                    "href": "https://api.digg.se/r2ps-api/v1/requests/d290f1ee-6c54-4b01-90e6-d701748f0851",
                    "rel": "self",
                    "method": "GET"
                },
                "poll": {
                    "href": "https://api.digg.se/r2ps-api/v1/requests/d290f1ee-6c54-4b01-90e6-d701748f0851",
                    "rel": "poll",
                    "method": "GET"
                }
            })))
            .build();
        ("Links", RefOr::T(Schema::Object(schema)))
    }
}

impl Links {
    /// Create a new `Links` with only a `self` link.
    pub fn new_with_self(href: impl Into<String>) -> Self {
        Self {
            self_link: Link::new(href, "GET").with_rel("self"),
            additional: HashMap::new(),
        }
    }

    /// Add an additional named link.
    pub fn add(mut self, name: impl Into<String>, link: Link) -> Self {
        self.additional.insert(name.into(), link);
        self
    }
}

impl Default for Links {
    fn default() -> Self {
        Self {
            self_link: Link::new("", "GET").with_rel("self"),
            additional: HashMap::new(),
        }
    }
}

/// Trait for resources that support HATEOAS.
pub trait HateoasResource {
    fn links(&self) -> &Links;
}
