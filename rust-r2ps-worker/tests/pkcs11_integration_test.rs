use digest::Digest;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use rdkafka::message::ToBytes;
use rust_r2ps_worker::application::hsm_spi_port::HsmSpiPort;
use rust_r2ps_worker::domain::Curve;
use rust_r2ps_worker::infrastructure::hsm_wrapper::{HsmWrapper, Pkcs11Config};
use rust_r2ps_worker::run;
use sha2::Sha256;
use tracing::info;

#[test]
fn gen_ecc_key() -> Result<(), Box<dyn std::error::Error>> {
    let config = config();
    let hsm_wrapper = HsmWrapper::new(config)?;
    let result = hsm_wrapper.generate_key(&"foobar", &Curve::P256)?;
    println!("{:?}", result);

    Ok(())
}

#[test]
fn gen_ecc_key_wrap_unwrap_sign() -> Result<(), Box<dyn std::error::Error>> {
    let config = config();
    let hsm_wrapper = HsmWrapper::new(config)?;
    let message = "foobar";
    let hsm_key_pair = hsm_wrapper.generate_key(message, &Curve::P256)?;

    let digest = Sha256::digest(message);
    let signature = hsm_wrapper.sign(&hsm_key_pair.wrapped_private_key, &digest.to_vec());

    let verifying_key = VerifyingKey::from_public_key_pem(&hsm_key_pair.public_key_pem)
        .map_err(|e| e.to_string())?;
    // Parse the signature (r || s, 64 bytes for P-256)
    let result = match Signature::from_slice(&signature.unwrap().to_bytes()) {
        Ok(signature) => verifying_key.verify(message.as_bytes(), &signature).is_ok(),
        Err(error) => {
            println!("{:?}", error);
            false
        }
    };

    assert!(result);
    Ok(())
}

fn config() -> Pkcs11Config {
    Pkcs11Config {
        lib_path: "/opt/homebrew/Cellar/softhsm/2.6.1/lib/softhsm/libsofthsm2.so".to_string(),
        slot_index: 0,
        user_pin: Some("123456".to_string()),
        so_pin: None,
        wrap_key_alias: String::from("aes-wrapping-key"),
    }
}
