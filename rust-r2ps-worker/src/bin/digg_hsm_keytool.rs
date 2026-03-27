use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clap::{Parser, Subcommand};
use cryptoki::object::ObjectClass;
use rust_r2ps_worker::application::port::outgoing::hsm_spi_port::HsmSpiPort;
use rust_r2ps_worker::domain::value_objects::r2ps::Curve;
use rust_r2ps_worker::infrastructure::adapters::outgoing::hsm_wrapper::{HsmWrapper, Pkcs11Config};
use rust_r2ps_worker::infrastructure::config::key_derivation;

#[derive(Parser)]
#[command(name = "digg-hsm-keytool", about = "Manage DIGG wallet HSM keys")]
struct Cli {
    #[arg(long)]
    hsm_lib: String,
    #[arg(long)]
    slot_token_label: String,
    #[arg(long)]
    pin: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create the AES-256 wrapping key used to protect generated EC keys at rest.
    CreateWrappingKey {
        #[arg(long)]
        label: String,
        /// Overwrite an existing wrapping key with the same label.
        #[arg(long)]
        force: bool,
    },
    /// Create a new P-256 root key in the HSM and verify it works for key derivation.
    CreateRootKey {
        #[arg(long)]
        label: String,
        /// Domain separator used to verify the derivation round-trip.
        #[arg(long, default_value = "jws-v1")]
        test_domain_sep: String,
        /// Overwrite an existing root key with the same label.
        #[arg(long)]
        force: bool,
    },
    /// Derive and print the JWS and OPAQUE public keys for the given root key and domain separators.
    DerivePublicKeys {
        #[arg(long)]
        root_key_label: String,
        #[arg(long, default_value = "jws-v1")]
        jws_domain_sep: String,
        #[arg(long, default_value = "opaque-v1")]
        opaque_domain_sep: String,
    },
    /// Remove wrapping key and root key from the HSM. Dry-run by default; pass --force to actually delete.
    Remove {
        #[arg(long)]
        wrap_key_label: String,
        #[arg(long)]
        root_key_label: String,
        /// Actually delete the keys (without this flag only a dry-run is performed).
        #[arg(long)]
        force: bool,
    },
    /// Check key presence and verify functional round-trips (wrap/sign and derivation).
    Status {
        #[arg(long)]
        wrap_key_label: String,
        #[arg(long)]
        root_key_label: String,
        /// Domain separator used in the derivation round-trip.
        #[arg(long, default_value = "jws-v1")]
        domain_sep: String,
        /// Print root key KID and derived key KID.
        #[arg(long)]
        verbose: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let hsm_lib = cli.hsm_lib;
    let slot_token_label = cli.slot_token_label;
    let pin = cli.pin;

    match cli.command {
        Command::CreateWrappingKey { label, force } => {
            // Open without wrap_key_alias so we can inspect before creating.
            let hsm_bare = open_hsm(
                hsm_lib.clone(),
                slot_token_label.clone(),
                pin.clone(),
                String::new(),
            );
            if hsm_bare
                .exists_by_label(ObjectClass::SECRET_KEY, &label)
                .expect("Failed to search HSM")
            {
                if !force {
                    eprintln!(
                        "Error: wrapping key '{}' already exists. Use --force to overwrite.",
                        label
                    );
                    std::process::exit(1);
                }
                hsm_bare
                    .destroy_objects_by_label(ObjectClass::SECRET_KEY, &label)
                    .expect("Failed to delete existing wrapping key");
                println!("Deleted existing wrapping key '{}'.", label);
            }
            hsm_bare
                .create_aes_wrapping_key(&label)
                .expect("Failed to create wrapping key");
            println!("Wrapping key '{}' ready.", label);
        }

        Command::CreateRootKey {
            label,
            test_domain_sep,
            force,
        } => {
            let hsm = open_hsm(hsm_lib, slot_token_label, pin, String::new());

            if hsm
                .exists_by_label(ObjectClass::SECRET_KEY, &label)
                .expect("Failed to search HSM")
            {
                if !force {
                    eprintln!(
                        "Error: root key '{}' already exists. Use --force to overwrite.",
                        label
                    );
                    std::process::exit(1);
                }
                hsm.destroy_objects_by_label(ObjectClass::SECRET_KEY, &label)
                    .expect("Failed to delete existing root key");
                println!("Deleted existing root key '{}'.", label);
            }

            println!("Creating root key '{}'...", label);
            hsm.create_hmac_root_key(&label)
                .expect("Failed to create root key in HSM");
            println!("Root key created.");

            println!(
                "Verifying derivation round-trip with domain sep '{}'...",
                test_domain_sep
            );
            let secret = derive_secret(&hsm, &label, &test_domain_sep);
            let kid = key_derivation::ec_kid_from_secret(&secret);
            println!("Derivation OK — test KID: {}", kid);
            println!();
            println!("Root key label: {}", label);
        }

        Command::Remove {
            wrap_key_label,
            root_key_label,
            force,
        } => {
            let hsm = open_hsm(hsm_lib, slot_token_label, pin, String::new());

            let to_delete: &[(&str, ObjectClass, &str)] = &[
                (
                    "wrapping key",
                    ObjectClass::SECRET_KEY,
                    wrap_key_label.as_str(),
                ),
                ("root key", ObjectClass::SECRET_KEY, root_key_label.as_str()),
            ];

            if !force {
                println!(
                    "Dry run — no keys will be deleted. Pass --force to actually remove them."
                );
                println!();
            }

            let mut any_found = false;
            for (desc, class, label) in to_delete {
                if hsm
                    .exists_by_label(*class, label)
                    .expect("Failed to query HSM")
                {
                    any_found = true;
                    if force {
                        hsm.destroy_objects_by_label(*class, label)
                            .expect("Failed to delete key");
                        println!("  deleted  {}: {}", desc, label);
                    } else {
                        println!("  would delete  {}: {}", desc, label);
                    }
                }
            }

            if !any_found {
                println!("No keys found.");
            }
        }

        Command::Status {
            wrap_key_label,
            root_key_label,
            domain_sep,
            verbose,
        } => {
            let mut all_ok = true;

            // Check existence in one connection, then drop before reopening for functional tests
            // (a second Pkcs11::initialize while one is live returns CryptokiAlreadyInitialized).
            let (wrap_exists, root_exists) = {
                let hsm = open_hsm(
                    hsm_lib.clone(),
                    slot_token_label.clone(),
                    pin.clone(),
                    String::new(),
                );
                let w = hsm
                    .exists_by_label(ObjectClass::SECRET_KEY, &wrap_key_label)
                    .expect("Failed to query HSM");
                let r = hsm
                    .exists_by_label(ObjectClass::SECRET_KEY, &root_key_label)
                    .expect("Failed to query HSM");
                (w, r)
            }; // hsm dropped here — PKCS#11 finalized

            report("wrapping key present", wrap_exists, &mut all_ok);
            if wrap_exists {
                let hsm = open_hsm(
                    hsm_lib.clone(),
                    slot_token_label.clone(),
                    pin.clone(),
                    wrap_key_label.clone(),
                );
                let result = check_wrap_sign(&hsm);
                report(
                    "  generate → wrap → sign → verify",
                    result.is_ok(),
                    &mut all_ok,
                );
                if let Err(ref e) = result {
                    eprintln!("    error: {}", e);
                }
            }

            report("root key present", root_exists, &mut all_ok);
            if root_exists {
                let hsm = open_hsm(hsm_lib, slot_token_label, pin, String::new());
                let result = check_derivation(&hsm, &root_key_label, &domain_sep);
                report("  derive → sign → verify", result.is_ok(), &mut all_ok);
                if let Err(ref e) = result {
                    eprintln!("    error: {}", e);
                }
                if verbose {
                    let secret = derive_secret(&hsm, &root_key_label, &domain_sep);
                    println!(
                        "    derived key KID: {}",
                        key_derivation::ec_kid_from_secret(&secret)
                    );
                }
            }

            if !all_ok {
                std::process::exit(1);
            }
        }

        Command::DerivePublicKeys {
            root_key_label,
            jws_domain_sep,
            opaque_domain_sep,
        } => {
            let hsm = open_hsm(hsm_lib, slot_token_label, pin, String::new());

            let jws_secret = derive_secret(&hsm, &root_key_label, &jws_domain_sep);
            let opaque_secret = derive_secret(&hsm, &root_key_label, &opaque_domain_sep);

            let jws_kid = key_derivation::ec_kid_from_secret(&jws_secret);
            let opaque_kid = key_derivation::ec_kid_from_secret(&opaque_secret);

            println!("JWS public key:");
            println!("  domain_sep : {}", jws_domain_sep);
            println!("  kid        : {}", jws_kid);
            println!(
                "  jwk        : {}",
                public_key_jwk_json(&jws_secret, &jws_kid)
            );
            println!();
            println!("OPAQUE public key:");
            println!("  domain_sep : {}", opaque_domain_sep);
            println!("  kid        : {}", opaque_kid);
            println!(
                "  jwk        : {}",
                public_key_jwk_json(&opaque_secret, &opaque_kid)
            );
        }
    }
}

fn open_hsm(
    hsm_lib: String,
    slot_token_label: String,
    pin: String,
    wrap_key_alias: String,
) -> HsmWrapper {
    HsmWrapper::new(Pkcs11Config {
        lib_path: hsm_lib,
        slot_token_label,
        so_pin: None,
        user_pin: Some(pin),
        wrap_key_alias,
    })
    .expect("Failed to initialize HSM")
}

fn derive_secret(hsm: &HsmWrapper, root_label: &str, domain_sep: &str) -> p256::SecretKey {
    let hmac_output = hsm
        .derive_key(root_label, domain_sep)
        .expect("HSM HMAC derivation failed");
    key_derivation::derive_scalar(hmac_output.as_ref(), domain_sep)
        .expect("Scalar derivation failed")
}

fn report(label: &str, ok: bool, all_ok: &mut bool) {
    if ok {
        println!("  [OK]   {}", label);
    } else {
        println!("  [FAIL] {}", label);
        *all_ok = false;
    }
}

/// Generate an ephemeral key, wrap it, sign a test payload, and verify the signature.
fn check_wrap_sign(hsm: &HsmWrapper) -> Result<(), Box<dyn std::error::Error>> {
    use p256::ecdsa::signature::hazmat::PrehashVerifier;
    use sha2::Digest;

    let key = hsm.generate_key("_status-check", &Curve::P256)?;

    let hash = sha2::Sha256::digest(b"digg-hsm-status-check");
    let sig_bytes = hsm.sign(&key, &hash)?;

    let x = URL_SAFE_NO_PAD.decode(&key.public_key_jwk.x)?;
    let y = URL_SAFE_NO_PAD.decode(&key.public_key_jwk.y)?;
    let mut point = vec![0x04u8];
    point.extend_from_slice(&x);
    point.extend_from_slice(&y);
    let vk = p256::ecdsa::VerifyingKey::from_sec1_bytes(&point).map_err(|e| e.to_string())?;
    let sig = p256::ecdsa::Signature::from_slice(&sig_bytes).map_err(|e| e.to_string())?;
    vk.verify_prehash(&hash, &sig).map_err(|e| e.to_string())?;

    Ok(())
}

/// Derive a key from the root key and verify it produces a valid signing key.
fn check_derivation(
    hsm: &HsmWrapper,
    root_label: &str,
    domain_sep: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::signature::Verifier;

    let secret = derive_secret(hsm, root_label, domain_sep);
    let signing_key = p256::ecdsa::SigningKey::from(&secret);
    let sig: p256::ecdsa::Signature = signing_key.sign(b"digg-hsm-status-check");
    signing_key
        .verifying_key()
        .verify(b"digg-hsm-status-check", &sig)
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn public_key_jwk_json(secret: &p256::SecretKey, kid: &str) -> String {
    use base64::Engine;
    use base64::prelude::BASE64_URL_SAFE_NO_PAD;
    use p256::elliptic_curve::sec1::ToEncodedPoint;

    let point = secret.public_key().as_affine().to_encoded_point(false);
    let x = BASE64_URL_SAFE_NO_PAD.encode(point.x().expect("x"));
    let y = BASE64_URL_SAFE_NO_PAD.encode(point.y().expect("y"));
    format!(
        r#"{{"kty":"EC","crv":"P-256","x":"{}","y":"{}","kid":"{}"}}"#,
        x, y, kid
    )
}
