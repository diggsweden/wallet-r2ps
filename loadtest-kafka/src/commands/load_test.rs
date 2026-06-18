// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Direct-to-Kafka load test.
//!
//! Concurrency model
//! -----------------
//! Every (client, slot) pair gets its own Tokio task. With `--clients C` and
//! `--inflight-per-client N` the load test holds up to `C × N` concurrent
//! auth+sign cycles. Tasks for the same `client_id` issue requests in
//! parallel — the worker's auth and sign ops emit `state: None`, so multiple
//! in-flight cycles never race on `state_jws`.
//!
//! The shared [`KafkaBackend`] holds one [`FutureProducer`]. With many tasks
//! producing concurrently, `linger.ms` + `batch.size` get genuine batch
//! pressure (vs. the single-VU baseline where each task waited on one ack
//! before sending the next message).
//!
//! Failure handling
//! ----------------
//! On cycle error, the task sleeps `--error-backoff-ms` (default 0) before
//! retrying. Default is no delay — match the user's "no delay between
//! requests" stance.

use anyhow::{Context, Result};
use rand::RngExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use integration_load_tests::backend::BackendClient;
use integration_load_tests::client::access_mechanism::{
    build_device_jwk, load_server_public_key_pem, AccessMechanismClient,
};
use integration_load_tests::model::test_data::TestDataEnvelope;
use integration_load_tests::stats::Stats;

use crate::cli::LoadTestArgs;
use crate::kafka::backend::{KafkaBackend, ProducerConfig};
use crate::runtime::boot;

pub async fn run(args: LoadTestArgs) -> Result<()> {
    let envelope = TestDataEnvelope::read_from(Path::new(&args.test_data))?;

    if envelope.clients.is_empty() {
        anyhow::bail!("Test data has no clients");
    }
    if envelope.clients.iter().any(|c| c.state_jws.is_none()) {
        anyhow::bail!(
            "Test data was produced by the REST tool — every client must have a \
             `state_jws`. Re-onboard with `loadtest-kafka generate`."
        );
    }

    let server_pubkey = load_server_public_key_pem(&args.server_pubkey_pem)?;

    let total_clients = if args.clients == 0 {
        envelope.clients.len()
    } else {
        args.clients.min(envelope.clients.len())
    };
    let total_tasks = total_clients * args.inflight_per_client;

    println!(
        "Load test (direct Kafka): {} clients × {} inflight = {} cycle tasks",
        total_clients, args.inflight_per_client, total_tasks
    );
    println!(
        "Test data: {}/{} clients from {}",
        total_clients,
        envelope.clients.len(),
        args.test_data
    );
    println!("Kafka: {}", args.bootstrap_servers);
    println!(
        "Producer: linger.ms={} batch.size={} compression={} acks={}",
        args.producer_linger_ms,
        args.producer_batch_size_bytes,
        args.producer_compression,
        args.producer_acks
    );
    if args.mean_delay_ms > 0 {
        println!("Mean delay: {}ms per loop", args.mean_delay_ms);
    } else {
        println!("Mode: no delay between requests");
    }
    if args.duration_secs > 0 {
        println!("Duration: {}s", args.duration_secs);
    } else {
        println!("Duration: unlimited (Ctrl+C to stop)");
    }

    let producer_cfg = ProducerConfig {
        linger_ms: args.producer_linger_ms,
        batch_size_bytes: args.producer_batch_size_bytes,
        compression: args.producer_compression.clone(),
        acks: args.producer_acks.clone(),
        ..ProducerConfig::default()
    };
    let runtime = boot(
        &args.bootstrap_servers,
        &args.broker_address_family,
        args.response_topic_partitions,
        Duration::from_secs(args.request_timeout_secs),
        producer_cfg,
    )
    .await
    .context("Failed to bring up Kafka runtime")?;

    for c in envelope.clients.iter().take(total_clients) {
        if let Some(s) = c.state_jws.as_deref() {
            runtime.backend.seed_state(&c.client_id, s);
        }
    }
    println!(
        "Response topics: {} (hsm), {} (state-init)",
        runtime.hsm_response_topic, runtime.state_init_response_topic
    );

    let backend: Arc<dyn BackendClient> = runtime.backend.clone();
    let kafka_backend = runtime.backend.clone();

    let running = Arc::new(AtomicBool::new(true));
    let stats = Arc::new(Stats::new());

    // Graceful shutdown
    let running_clone = Arc::clone(&running);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\nStopping...");
        running_clone.store(false, Ordering::Relaxed);
    });
    if args.duration_secs > 0 {
        let running_clone = Arc::clone(&running);
        let duration = args.duration_secs;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(duration)).await;
            running_clone.store(false, Ordering::Relaxed);
        });
    }

    let stats_clone = Arc::clone(&stats);
    let running_stats = Arc::clone(&running);
    let stats_interval = args.stats_interval_secs;
    tokio::spawn(async move {
        while running_stats.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(stats_interval)).await;
            if running_stats.load(Ordering::Relaxed) {
                stats_clone.print_summary();
            }
        }
    });

    // Per-(client, slot) task spawn. Build the AccessMechanismClient once
    // per client and share it across all that client's parallel loops.
    let server_pubkey = Arc::new(server_pubkey);
    let mut handles = Vec::with_capacity(total_tasks);
    for c in envelope.clients.iter().take(total_clients) {
        let device_jwk =
            build_device_jwk(&c.device_key.x, &c.device_key.y, &c.device_key.d, &c.kid)
                .with_context(|| format!("build device JWK for {}", c.client_id))?;
        let am = Arc::new(AccessMechanismClient::new(
            Arc::clone(&backend),
            (*server_pubkey).clone(),
            device_jwk,
            c.kid.clone(),
            c.pin_stretch_d.clone(),
            envelope.opaque_context.clone(),
            envelope.opaque_server_identifier.clone(),
        ));
        for slot in 0..args.inflight_per_client {
            let am = Arc::clone(&am);
            let pin = c.pin.clone();
            let client_id = c.client_id.clone();
            let hsm_kid = c.hsm_kid.clone();
            let stats = Arc::clone(&stats);
            let running = Arc::clone(&running);
            let args = args.clone();
            let kafka_backend = Arc::clone(&kafka_backend);
            handles.push(tokio::spawn(async move {
                if args.produce_only {
                    produce_only_loop(
                        slot,
                        &am,
                        &pin,
                        &client_id,
                        &kafka_backend,
                        &stats,
                        &running,
                    )
                    .await;
                } else {
                    cycle_loop(
                        slot, &am, &pin, &client_id, &hsm_kid, &args, &stats, &running,
                    )
                    .await;
                }
            }));
        }
    }

    for handle in handles {
        let _ = handle.await;
    }

    stats.print_report();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cycle_loop(
    slot: usize,
    am: &AccessMechanismClient,
    pin: &str,
    client_id: &str,
    hsm_kid: &str,
    args: &LoadTestArgs,
    stats: &Stats,
    running: &AtomicBool,
) {
    while running.load(Ordering::Relaxed) {
        match run_one_cycle(am, pin, client_id, hsm_kid, args, stats, running).await {
            Ok(()) => {}
            Err(e) => {
                stats.record_auth_error();
                eprintln!(
                    "[client {}.{}] cycle error: {:#}",
                    &client_id[..12.min(client_id.len())],
                    slot,
                    e
                );
                if args.error_backoff_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(args.error_backoff_ms)).await;
                }
            }
        }
    }
}

async fn run_one_cycle(
    am: &AccessMechanismClient,
    pin: &str,
    client_id: &str,
    hsm_kid: &str,
    args: &LoadTestArgs,
    stats: &Stats,
    running: &AtomicBool,
) -> Result<()> {
    poisson_delay(args.mean_delay_ms).await;
    let t0 = Instant::now();
    let (session_key, session_id) = am.create_session(pin, client_id).await?;
    stats.record_latency(t0.elapsed().as_millis() as u64);
    stats.record_auth_cycle();

    for _ in 0..args.signs_per_cycle {
        if !running.load(Ordering::Relaxed) {
            break;
        }
        poisson_delay(args.mean_delay_ms).await;

        let buf: [u8; 32] = rand::rng().random();
        let t_sign = Instant::now();
        match am
            .hsm_sign(&session_key, &session_id, client_id, hsm_kid, &buf)
            .await
        {
            Ok(_) => stats.record_latency(t_sign.elapsed().as_millis() as u64),
            Err(e) => {
                stats.record_error();
                eprintln!("HSM sign error: {:#}", e);
            }
        }
    }

    Ok(())
}

/// Producer-throughput benchmark: fires `hsm-requests` messages with a
/// pre-built OPAQUE KE1 JWS, mutating only the envelope's `request_id` per
/// message. The hot loop pays no ECDSA sign, no OPAQUE blind, no hash-to-
/// curve — only the envelope serialise + Kafka enqueue.
///
/// The reused JWS bytes won't pass replay protection at the worker, but
/// this loop is a pure producer-side benchmark: we measure how fast the
/// client + Kafka can absorb writes, independent of the worker accepting
/// them.
#[allow(clippy::too_many_arguments)]
async fn produce_only_loop(
    slot: usize,
    am: &AccessMechanismClient,
    pin: &str,
    client_id: &str,
    kafka_backend: &KafkaBackend,
    stats: &Stats,
    running: &AtomicBool,
) {
    // Build the KE1 JWS ONCE. OPAQUE login_start uses fresh randomness, but
    // this is a producer benchmark — we don't need each message to verify
    // server-side; we need the client to keep producing.
    let jws = match am.build_login_start_jws(pin) {
        Ok(j) => j,
        Err(e) => {
            stats.record_error();
            eprintln!("[client {}.{}] build template JWS failed: {:#}", client_id, slot, e);
            return;
        }
    };
    while running.load(Ordering::Relaxed) {
        let t0 = Instant::now();
        if let Err(e) = kafka_backend.fire_hsm_request(client_id, &jws) {
            stats.record_error();
            eprintln!("[client {}.{}] enqueue failed: {:#}", client_id, slot, e);
            tokio::task::yield_now().await;
            continue;
        }
        stats.record_latency(t0.elapsed().as_millis() as u64);
        tokio::task::yield_now().await;
    }
}

async fn poisson_delay(mean_ms: u64) {
    if mean_ms == 0 {
        return;
    }
    let u: f64 = rand::rng().random_range(0.0..1.0_f64);
    let exp = -u.ln() * mean_ms as f64;
    let clamped = exp.min((mean_ms * 5) as f64) as u64;
    if clamped > 0 {
        tokio::time::sleep(Duration::from_millis(clamped)).await;
    }
}
