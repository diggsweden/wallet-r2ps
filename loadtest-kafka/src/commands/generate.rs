// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Onboard clients by talking directly to the request/response Kafka topics.
//!
//! Each client goes through the same OPAQUE + JWS/JWE flow as the REST-based
//! generator in `integration-load-tests`, but every round-trip is a Kafka
//! produce + consume instead of an HTTP request to the BFF.
//!
//! The resulting test-data envelope is fully compatible with the REST tool;
//! the only difference is that this generator records the final `state_jws`,
//! which the load-test command needs to include in every `HsmWorkerRequest`.

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use integration_load_tests::backend::BackendClient;
use integration_load_tests::client::access_mechanism::{
    build_device_jwk, load_server_public_key_pem, AccessMechanismClient,
};
use integration_load_tests::crypto::keygen::{self, EcKeyPair};
use integration_load_tests::model::test_data::{ClientTestData, DeviceKey, TestDataEnvelope};
use integration_load_tests::protocol::types::EcPublicJwk;

use crate::cli::GenerateArgs;
use crate::kafka::backend::KafkaBackend;
use crate::runtime::boot;

pub async fn run(args: GenerateArgs) -> Result<()> {
    let server_pubkey = load_server_public_key_pem(&args.server_pubkey_pem)?;

    println!(
        "Generating {} test clients via Kafka ({})",
        args.count, args.bootstrap_servers
    );
    println!("Concurrency: {}, PIN: {}", args.concurrency, args.pin);

    let runtime = boot(
        &args.bootstrap_servers,
        &args.broker_address_family,
        args.response_topic_partitions,
        Duration::from_secs(30),
        crate::kafka::backend::ProducerConfig::default(),
    )
    .await?;

    let backend: Arc<dyn BackendClient> = runtime.backend.clone();
    let kafka_backend = runtime.backend.clone();

    let semaphore = Arc::new(Semaphore::new(args.concurrency));
    let mut handles = Vec::with_capacity(args.count);

    for i in 0..args.count {
        let backend = Arc::clone(&backend);
        let kafka_backend = Arc::clone(&kafka_backend);
        let sem = Arc::clone(&semaphore);
        let server_pk = server_pubkey.clone();
        let args = args.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            generate_one_client(backend, kafka_backend, &server_pk, &args, i).await
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
        if (completed + failed) % args.concurrency.max(1) == 0
            || completed + failed == args.count
        {
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
    backend: Arc<dyn BackendClient>,
    kafka_backend: Arc<KafkaBackend>,
    server_pubkey: &josekit::jwk::Jwk,
    args: &GenerateArgs,
    index: usize,
) -> Result<ClientTestData> {
    let device_key = keygen::generate_ec_p256_keypair();
    let pin_stretch_key = keygen::generate_ec_p256_keypair();

    let device_jwk =
        build_device_jwk(&device_key.x, &device_key.y, &device_key.d, &device_key.kid)?;

    let public_key = EcPublicJwk {
        kty: "EC".to_string(),
        crv: "P-256".to_string(),
        x: device_key.x.clone(),
        y: device_key.y.clone(),
        kid: device_key.kid.clone(),
    };

    let am = AccessMechanismClient::new(
        backend,
        server_pubkey.clone(),
        device_jwk,
        device_key.kid.clone(),
        pin_stretch_key.d.clone(),
        args.opaque_context.clone(),
        args.opaque_server_id.clone(),
    );

    let (client_id, auth_code) = am.init_state(&public_key, &args.ttl).await?;
    tracing::debug!("Client {}: initialized, client_id={}", index, client_id);

    let _ = am.register_pin(&args.pin, &client_id, &auth_code).await?;
    let (session_key, session_id) = am.create_session(&args.pin, &client_id).await?;
    let hsm_kid = am
        .hsm_generate_key(&session_key, &session_id, &client_id)
        .await?;
    tracing::debug!("Client {}: HSM key generated, kid={}", index, hsm_kid);

    // Capture the current state_jws so the load-test command can re-seed it
    // into the backend without re-running onboarding.
    let state_jws = kafka_backend.snapshot_state(&client_id);

    Ok(ClientTestData {
        client_id,
        kid: device_key.kid.clone(),
        pin: args.pin.clone(),
        pin_stretch_d: pin_stretch_key.d.clone(),
        device_key: device_key_to_model(&device_key),
        hsm_kid,
        state_jws,
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
