#[derive(Debug, Clone)]
pub struct OpaqueConfig {
    pub opaque_server_setup: Option<String>,
    pub opaque_context: String,
    pub opaque_server_identifier: String,
}
