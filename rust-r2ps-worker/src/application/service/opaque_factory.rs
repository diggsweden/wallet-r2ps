use argon2::password_hash::rand_core::OsRng;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use opaque_ke::ServerSetup;
use opaque_ke::keypair::{KeyPair, PrivateKey, PublicKey};
use p256::NistP256;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use pem::Pem;
use tracing::info;

use crate::domain::DefaultCipherSuite;

pub fn init_server_setup(
    opaque_server_setup: &Option<String>,
    server_private_key: &Pem,
) -> ServerSetup<DefaultCipherSuite> {
    match load_server_setup(opaque_server_setup) {
        Ok(setup) => setup,
        Err(_e) => {
            let setup = create_server_setup(server_private_key)
                .expect("Failed to create opaque server setup");
            info!(
                "OPAQUE_SERVER_SETUP={}",
                BASE64_STANDARD.encode(setup.serialize())
            );
            setup
        }
    }
}

fn create_server_setup(
    server_private_key_pem: &Pem,
) -> Result<ServerSetup<DefaultCipherSuite>, String> {
    let secret_key = p256::SecretKey::from_pkcs8_pem(&pem::encode(server_private_key_pem))
        .map_err(|e| format!("Failed to parse P-256 private key: {:?}", e))?;

    let keypair = KeyPair::new(
        PrivateKey::<NistP256>::deserialize(&secret_key.to_bytes())
            .map_err(|e| format!("Failed to deserialize private key: {:?}", e))?,
        PublicKey::<NistP256>::deserialize(
            secret_key
                .public_key()
                .as_affine()
                .to_encoded_point(true)
                .as_bytes(),
        )
        .map_err(|e| format!("Failed to deserialize public key: {:?}", e))?,
    );

    Ok(ServerSetup::<DefaultCipherSuite>::new_with_key_pair(
        &mut OsRng, keypair,
    ))
}

fn load_server_setup(
    server_setup: &Option<String>,
) -> Result<ServerSetup<DefaultCipherSuite>, String> {
    match server_setup {
        Some(server_setup_hex) => {
            let bytes = BASE64_STANDARD
                .decode(server_setup_hex.as_bytes())
                .map_err(|e| format!("Failed to decode server setup hex: {}", e))?;

            // Deserialize from bytes
            ServerSetup::deserialize(&bytes)
                .map_err(|e| format!("Failed to deserialize server setup: {}", e))
        }
        None => Err("Invalid server setup".to_string()),
    }
}
