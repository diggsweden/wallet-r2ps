use std::env;
use cryptoki::{
    context::Pkcs11,
    object::{Attribute, ObjectHandle},
    mechanism::Mechanism,
    session::Session,

};
use cryptoki::error::Error;
use crate::domain::Curve;
use crate::infrastructure::hsm_wrapper::HsmKey;

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
    fn generate_key(&self, label: &str, curve: &Curve) -> Result<HsmKey, Box<dyn std::error::Error>>;
    fn sign(&self, wrapped_key: &Vec<u8>, sign_payload: &Vec<u8>) -> Result<Vec<u8>, Error>;
}