use crate::domain::DefaultCipherSuite;
use crate::infrastructure::hsm_wrapper::HsmKey;
use generic_array::GenericArray;
use opaque_ke::ServerRegistrationLen;
use pem::Pem;

#[derive(Debug, Clone)]
pub struct ClientMetadata {
    pub client_id: String,
    pub wallet_id: String,
    pub client_public_key: Pem,
    pub password_file: Option<GenericArray<u8, ServerRegistrationLen<DefaultCipherSuite>>>,
    pub keys: Vec<HsmKey>,
}
