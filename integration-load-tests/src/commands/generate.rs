// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Test data generation command.
//!
//! For each client:
//!   1. Generate EC P-256 device key pair + PIN stretch key pair
//!   2. POST /device-states -> get clientId + authorizationCode
//!   3. OPAQUE registration (start + finish via POST /)
//!   4. OPAQUE login (start + finish via POST /) -> get session key
//!   5. HSM generate key (via POST /) -> get hsm_kid
//!   6. Save test data to gzip JSON file

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::cli::GenerateArgs;
use crate::client::access_mechanism::{
    build_device_jwk, load_server_public_key_pem, AccessMechanismClient,
};
use crate::client::rest_client::RestClient;
use crate::crypto::keygen::{self, EcKeyPair};
use crate::model::test_data::{ClientTestData, DeviceKey, TestDataEnvelope};
use crate::protocol::types::EcPublicJwk;

pub async fn run(args: GenerateArgs) -> Result<()> {
    let server_pubkey = load_server_public_key_pem(&args.server_pubkey_pem)?;
    let rest: Arc<RestClient> = Arc::new(RestClient::new(&args.bff_url)?);
    let semaphore = Arc::new(Semaphore::new(args.concurrency));

    println!(
        "Generating {} test clients via {}",
        args.count, args.bff_url
    );
    println!("Concurrency: {}, PIN: {}", args.concurrency, args.pin);

    let mut handles = Vec::with_capacity(args.count);

    for i in 0..args.count {
        let rest = Arc::clone(&rest);
        let sem = Arc::clone(&semaphore);
        let server_pk = server_pubkey.clone();
        let args = args.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            generate_one_client(rest, &server_pk, &args, i).await
        });
        handles.push(handle);
    }

    let mut clients = Vec::with_capacity(args.count);
    let mut completed = 0;
    let mut failed = 0;

    for handle in handles {
        match handle.await? {
            Ok(client) => {
                clients.push(client);
                completed += 1;
            }
            Err(e) => {
                eprintln!("Client generation failed: {:#}", e);
                failed += 1;
            }
        }
        if (completed + failed) % args.concurrency.max(1) == 0 || completed + failed == args.count {
            println!(
                "  Progress: {}/{} (failed: {})",
                completed, args.count, failed
            );
        }
    }

    let envelope = TestDataEnvelope {
        opaque_context: args.opaque_context,
        opaque_server_identifier: args.opaque_server_id,
        clients,
    };

    let output_path = Path::new(&args.output);
    envelope.write_gzip(output_path)?;

    println!(
        "\nDone: {} clients written to {} (failed: {})",
        completed, args.output, failed,
    );

    Ok(())
}

async fn generate_one_client(
    rest: Arc<RestClient>,
    server_pubkey: &josekit::jwk::Jwk,
    args: &GenerateArgs,
    index: usize,
) -> Result<ClientTestData> {
    // 1. Generate key pairs
    let device_key = keygen::generate_ec_p256_keypair();
    let pin_stretch_key = keygen::generate_ec_p256_keypair();

    let device_jwk =
        build_device_jwk(&device_key.x, &device_key.y, &device_key.d, &device_key.kid)?;

    let public_key = EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: device_key.x.clone(),
        y: device_key.y.clone(),
        kid: Some(device_key.kid.clone()),
    };

    let am = AccessMechanismClient::new(
        rest,
        server_pubkey.clone(),
        device_jwk,
        device_key.kid.clone(),
        pin_stretch_key.d.clone(),
        args.opaque_context.clone(),
        args.opaque_server_id.clone(),
    );

    // 2. Init state
    let (client_id, auth_code) = am.init_state(&public_key, &args.ttl).await?;
    tracing::debug!("Client {}: initialized, client_id={}", index, client_id);

    // 3. Register PIN
    let _export_key = am.register_pin(&args.pin, &client_id, &auth_code).await?;
    tracing::debug!("Client {}: PIN registered", index);

    // 4. Create session (login)
    let (session_key, session_id) = am.create_session(&args.pin, &client_id).await?;
    tracing::debug!("Client {}: session created", index);

    // 5. Generate HSM key
    let hsm_kid = am
        .hsm_generate_key(&session_key, &session_id, &client_id)
        .await?;
    tracing::debug!("Client {}: HSM key generated, kid={}", index, hsm_kid);

    Ok(ClientTestData {
        client_id,
        kid: device_key.kid.clone(),
        pin: args.pin.clone(),
        pin_stretch_d: pin_stretch_key.d.clone(),
        device_key: device_key_to_model(&device_key),
        hsm_kid,
    })
}

fn device_key_to_model(kp: &EcKeyPair) -> DeviceKey {
    DeviceKey {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: kp.x.clone(),
        y: kp.y.clone(),
        d: kp.d.clone(),
        kid: kp.kid.clone(),
    }
}
