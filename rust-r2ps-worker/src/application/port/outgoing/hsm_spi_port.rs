use crate::domain::Curve;
use crate::infrastructure::hsm_wrapper::HsmKey;
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

pub trait HsmSpiPort {
    fn generate_key(
        &self,
        label: &str,
        curve: &Curve,
    ) -> Result<HsmKey, Box<dyn std::error::Error>>;
    fn sign(&self, wrapped_key: &Vec<u8>, sign_payload: &Vec<u8>) -> Result<Vec<u8>, Error>;
}
