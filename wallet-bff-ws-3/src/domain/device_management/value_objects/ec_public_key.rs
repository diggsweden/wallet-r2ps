/// EC public key in JWK format (P-256 curve).
///
/// Used during device initialization to register a new device.
/// This is a value object that validates the key parameters.
///
/// The `kid` (key identifier) field is required per RFC 7517 and is used by
/// the HSM worker to identify which device key to use for signature verification
/// when processing subsequent service requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcPublicKey {
    kty: String,
    crv: String,
    x: String,
    y: String,
    kid: String,
}

impl EcPublicKey {
    /// Create a new EC public key, validating the parameters.
    ///
    /// The `kid` field is required and must not be empty. It identifies this
    /// key in the device's key set and is used by the HSM worker to look up
    /// the correct public key for JWS signature verification.
    pub fn new(
        kty: impl Into<String>,
        crv: impl Into<String>,
        x: impl Into<String>,
        y: impl Into<String>,
        kid: impl Into<String>,
    ) -> Result<Self, EcPublicKeyError> {
        let kty = kty.into();
        let crv = crv.into();
        let x = x.into();
        let y = y.into();
        let kid = kid.into();

        if kty != "EC" {
            return Err(EcPublicKeyError::InvalidKeyType(kty));
        }
        if crv != "P-256" {
            return Err(EcPublicKeyError::InvalidCurve(crv));
        }
        if x.is_empty() {
            return Err(EcPublicKeyError::MissingCoordinate("x".to_string()));
        }
        if y.is_empty() {
            return Err(EcPublicKeyError::MissingCoordinate("y".to_string()));
        }
        if kid.is_empty() {
            return Err(EcPublicKeyError::MissingKid);
        }

        Ok(Self {
            kty,
            crv,
            x,
            y,
            kid,
        })
    }

    pub fn kty(&self) -> &str {
        &self.kty
    }

    pub fn crv(&self) -> &str {
        &self.crv
    }

    pub fn x(&self) -> &str {
        &self.x
    }

    pub fn y(&self) -> &str {
        &self.y
    }

    /// Key identifier (RFC 7517). Used by the HSM worker to look up
    /// the device public key for signature verification.
    pub fn kid(&self) -> &str {
        &self.kid
    }
}

/// Errors that can occur when creating an `EcPublicKey`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EcPublicKeyError {
    #[error("invalid key type: expected 'EC', got '{0}'")]
    InvalidKeyType(String),

    #[error("invalid curve: expected 'P-256', got '{0}'")]
    InvalidCurve(String),

    #[error("missing coordinate: {0}")]
    MissingCoordinate(String),

    #[error("missing key identifier (kid)")]
    MissingKid,
}
