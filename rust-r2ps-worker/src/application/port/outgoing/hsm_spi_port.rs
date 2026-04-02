use crate::define_byte_vector;
use crate::domain::Curve;
use crate::domain::HsmKey;
use cryptoki::error::Error;

pub struct KeyGenParams {
    pub label: String,
    pub curve_oid: Vec<u8>,
}

pub struct KeyProviderInfo {
    pub pin: String,
}

pub struct EcKeyPairRecord {
    pub private_key_data: Vec<u8>,
}

define_byte_vector!(
    /// Raw output from the HSM key derivation operation, used as IKM for HKDF-based key derivation.
    DerivedSecret,
    8
);

#[cfg_attr(test, mockall::automock)]
pub trait HsmSpiPort {
    fn generate_key(
        &self,
        label: &str,
        curve: &Curve,
    ) -> Result<HsmKey, Box<dyn std::error::Error>>;

    fn sign(&self, key: &HsmKey, sign_payload: &[u8]) -> Result<Vec<u8>, Error>;

    /// Derives secret material from the named root key and domain separator.
    /// Returns raw bytes used as IKM for HKDF-based key derivation.
    fn derive_key(
        &self,
        root_key_label: &str,
        domain_separator: &str,
    ) -> Result<DerivedSecret, Error>;
}
