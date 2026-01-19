use crate::domain::Curve;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmKey {
    pub wrapped_private_key: Vec<u8>,
    pub public_key_jwk: EcPublicJwk,
    pub curve_name: Curve,
    pub creation_time: chrono::DateTime<chrono::Utc>,
}

impl HsmKey {
    pub fn kid(&self) -> &str {
        &self.public_key_jwk.kid
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EcPublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    pub kid: String,
}
