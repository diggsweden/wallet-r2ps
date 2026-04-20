// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use digest::Digest;
use hsm_worker::application::hsm_spi_port::HsmSpiPort;
use hsm_worker::domain::Curve;
use hsm_worker::infrastructure::config::app_config::AppConfig;
use hsm_worker::infrastructure::hsm_wrapper::{HsmWrapper, Pkcs11Config};
use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};
use rdkafka::message::ToBytes;
use sha2::Sha256;
use std::sync::{Mutex, OnceLock};

static HSM_INSTANCE: OnceLock<Mutex<HsmWrapper>> = OnceLock::new();

fn get_hsm() -> &'static Mutex<HsmWrapper> {
    HSM_INSTANCE.get_or_init(|| {
        dotenvy::dotenv().ok();

        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let app_config = AppConfig::new().unwrap();
        let wrapper = HsmWrapper::new(Pkcs11Config {
            lib_path: app_config.pkcs11_lib,
            slot_token_label: app_config.pkcs11_slot_token_label,
            so_pin: app_config.pkcs11_so_pin,
            user_pin: app_config.pkcs11_user_pin,
            wrap_key_alias: app_config.pkcs11_wrap_key_alias,
        })
        .expect("Failed to initialize HSM Wrapper");
        Mutex::new(wrapper)
    })
}

#[test]
#[ignore]
fn gen_ecc_key() -> Result<(), Box<dyn std::error::Error>> {
    let hsm_wrapper = get_hsm().lock()?;
    let result = hsm_wrapper.generate_key("foobar", &Curve::P256)?;
    println!("{:?}", result);

    Ok(())
}

#[test]
#[ignore]
fn gen_ecc_key_wrap_unwrap_sign() -> Result<(), Box<dyn std::error::Error>> {
    let hsm_wrapper = get_hsm().lock()?;
    let message = "foobar";
    let hsm_key = hsm_wrapper.generate_key(message, &Curve::P256)?;
    let digest = Sha256::digest(message);
    let signature = hsm_wrapper.sign(&hsm_key, &digest);

    let ec_public_jwk = &hsm_key.public_key_jwk;
    let x_bytes = BASE64_URL_SAFE_NO_PAD.decode(&ec_public_jwk.x)?;
    let y_bytes = BASE64_URL_SAFE_NO_PAD.decode(&ec_public_jwk.y)?;

    let mut sec1_bytes = vec![0x04];
    sec1_bytes.extend_from_slice(&x_bytes);
    sec1_bytes.extend_from_slice(&y_bytes);

    let verifying_key = VerifyingKey::from_sec1_bytes(&sec1_bytes).map_err(|e| e.to_string())?;

    // Parse the signature (r || s, 64 bytes for P-256)
    let result = match Signature::from_slice(signature?.to_bytes()) {
        Ok(signature) => verifying_key.verify_prehash(&digest, &signature).is_ok(),
        Err(error) => {
            println!("{:?}", error);
            false
        }
    };

    assert!(result);
    Ok(())
}
