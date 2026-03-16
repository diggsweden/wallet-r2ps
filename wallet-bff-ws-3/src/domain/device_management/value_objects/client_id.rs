use std::fmt;

/// Unique identifier for a device/wallet.
///
/// Enforces that the identifier is non-empty and contains only valid characters.
/// In production, this would be generated as a UUID v4.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientId(String);

impl ClientId {
    /// Create a new `ClientId`, validating that it is non-empty.
    pub fn new(value: impl Into<String>) -> Result<Self, ClientIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ClientIdError::Empty);
        }
        Ok(Self(value))
    }

    /// Generate a new random `ClientId` using UUID v4.
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Return the inner string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume self and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for ClientId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when creating a `ClientId`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ClientIdError {
    #[error("client ID must not be empty")]
    Empty,
}
