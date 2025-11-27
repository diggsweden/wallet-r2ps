use pem::Pem;

#[derive(Debug, Clone)]
pub struct ClientMetadata {
    pub client_id: String,
    pub wallet_id: String,
    pub client_public_key: Pem,
    pub password_file: Option<Vec<u8>>,
}
