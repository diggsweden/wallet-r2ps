use digest::Digest;
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use rdkafka::message::ToBytes;
use rust_r2ps_worker::application::hsm_spi_port::HsmSpiPort;
use rust_r2ps_worker::domain::Curve;
use rust_r2ps_worker::infrastructure::hsm_wrapper::{HsmWrapper, Pkcs11Config};
use sha2::Sha256;

#[test]
fn gen_ecc_key() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let config = Pkcs11Config::new_from_env()?;
    let hsm_wrapper = HsmWrapper::new(config)?;
    let result = hsm_wrapper.generate_key(&"foobar", &Curve::P256)?;
    println!("{:?}", result);

    Ok(())
}

#[test]
fn gen_ecc_key_wrap_unwrap_sign() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let config = Pkcs11Config::new_from_env()?;
    let hsm_wrapper = HsmWrapper::new(config)?;
    let message = "foobar";
    let hsm_key_pair = hsm_wrapper.generate_key(message, &Curve::P256)?;

    let digest = Sha256::digest(message);
    let signature = hsm_wrapper.sign(&hsm_key_pair.wrapped_private_key, &digest.to_vec());

    let verifying_key = VerifyingKey::from_public_key_pem(&hsm_key_pair.public_key_pem)
        .map_err(|e| e.to_string())?;
    // Parse the signature (r || s, 64 bytes for P-256)
    let result = match Signature::from_slice(&signature.unwrap().to_bytes()) {
        Ok(signature) => verifying_key.verify_prehash(&digest, &signature).is_ok(),
        Err(error) => {
            println!("{:?}", error);
            false
        }
    };

    assert!(result);
    Ok(())
}
