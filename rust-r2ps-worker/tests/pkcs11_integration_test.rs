use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use digest::Digest;
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};
use rdkafka::message::ToBytes;
use rust_r2ps_worker::application::hsm_spi_port::HsmSpiPort;
use rust_r2ps_worker::domain::Curve;
use rust_r2ps_worker::infrastructure::hsm_wrapper::{HsmWrapper, Pkcs11Config};
use sha2::Sha256;
use std::sync::{Mutex, OnceLock};

static HSM_INSTANCE: OnceLock<Mutex<HsmWrapper>> = OnceLock::new();

fn get_hsm() -> &'static Mutex<HsmWrapper> {
    HSM_INSTANCE.get_or_init(|| {
        dotenvy::dotenv().ok();
        let config = Pkcs11Config::new_from_env().expect("Failed to load PKCS11 config from env");
        let wrapper = HsmWrapper::new(config).expect("Failed to initialize HSM Wrapper");
        Mutex::new(wrapper)
    })
}

#[test]
fn gen_ecc_key() -> Result<(), Box<dyn std::error::Error>> {
    let hsm_wrapper = get_hsm().lock()?;
    let result = hsm_wrapper.generate_key(&"foobar", &Curve::P256)?;
    println!("{:?}", result);

    Ok(())
}

#[test]
fn gen_ecc_key_wrap_unwrap_sign() -> Result<(), Box<dyn std::error::Error>> {
    let hsm_wrapper = get_hsm().lock()?;
    let message = "foobar";
    let hsm_key = hsm_wrapper.generate_key(message, &Curve::P256)?;

    let digest = Sha256::digest(message);
    let signature = hsm_wrapper.sign(&hsm_key.wrapped_private_key, &digest.to_vec());

    let ec_public_jwk = &hsm_key.public_key_jwk;
    let x_bytes = BASE64_URL_SAFE_NO_PAD.decode(&ec_public_jwk.x)?;
    let y_bytes = BASE64_URL_SAFE_NO_PAD.decode(&ec_public_jwk.y)?;

    let mut sec1_bytes = vec![0x04];
    sec1_bytes.extend_from_slice(&x_bytes);
    sec1_bytes.extend_from_slice(&y_bytes);

    let verifying_key = VerifyingKey::from_sec1_bytes(&sec1_bytes).map_err(|e| e.to_string())?;

    // Parse the signature (r || s, 64 bytes for P-256)
    let result = match Signature::from_slice(&signature?.to_bytes()) {
        Ok(signature) => verifying_key.verify_prehash(&digest, &signature).is_ok(),
        Err(error) => {
            println!("{:?}", error);
            false
        }
    };

    assert!(result);
    Ok(())
}
