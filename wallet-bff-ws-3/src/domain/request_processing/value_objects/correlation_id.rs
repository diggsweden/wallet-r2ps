use std::fmt;
use uuid::Uuid;

/// Unique identifier for correlating a service request with its response.
///
/// Generated as a UUID v4 when a new service request is submitted.
/// Used to track the request lifecycle across the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId(Uuid);

impl CorrelationId {
    /// Generate a new random correlation ID.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a `CorrelationId` from an existing UUID.
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Return the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for CorrelationId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<CorrelationId> for Uuid {
    fn from(id: CorrelationId) -> Self {
        id.0
    }
}
