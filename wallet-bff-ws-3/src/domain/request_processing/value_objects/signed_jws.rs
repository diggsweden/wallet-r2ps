use std::fmt;

/// A JWS-signed payload.
///
/// Value object representing any JWS-signed string in the system.
/// Used for outer request JWS and service response JWS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedJws(String);

impl SignedJws {
    /// Create a new `SignedJws`, validating that it is non-empty.
    pub fn new(jws: impl Into<String>) -> Result<Self, SignedJwsError> {
        let jws = jws.into();
        if jws.is_empty() {
            return Err(SignedJwsError::Empty);
        }
        Ok(Self(jws))
    }

    /// Return the JWS string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume self and return the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for SignedJws {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.len() > 20 {
            write!(f, "SignedJws({}...)", &self.0[..20])
        } else {
            write!(f, "SignedJws({})", &self.0)
        }
    }
}

/// Errors that can occur when creating a `SignedJws`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SignedJwsError {
    #[error("JWS string must not be empty")]
    Empty,
}
