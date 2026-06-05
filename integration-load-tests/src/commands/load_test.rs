// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Load test command with two pacing modes.
//!
//! Closed-loop (--threads, default): N tokio tasks each loop on
//! sequential cycles. Inter-arrival within each task is Poisson via
//! --mean-delay-ms. Concurrency = N (not throughput-driven).
//!
//! Open-loop (--target-rps > 0): one rate-paced producer fires cycles
//! at `target_rps` cycles/sec, spawning each as an independent tokio
//! task. A bounded `Semaphore` caps in-flight to `--max-concurrent`;
//! when the cap is hit the producer records a saturation tick and
//! drops the request, which surfaces the SUT's actual throughput
//! ceiling instead of self-throttling like closed-loop does. A single
//! loadtest pod with a couple of CPU cores can drive thousands of rps
//! this way because each cycle just awaits I/O.
//!
//! Each cycle:
//!   1. Pick a random client from test data
//!   2. OPAQUE login (start + finish) -> get session key
//!   3. Perform N HSM sign operations

use anyhow::Result;
use rand::RngExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::cli::LoadTestArgs;
use crate::client::access_mechanism::{
    build_device_jwk, load_server_public_key_pem, AccessMechanismClient,
};
use crate::client::rest_client::RestClient;
use crate::model::test_data::TestDataEnvelope;
use crate::stats::Stats;

/// Pre-built client pool shared across spawned cycles.
type ClientPool = Vec<(AccessMechanismClient, String, String, String)>;

pub async fn run(args: LoadTestArgs) -> Result<()> {
    let envelope = TestDataEnvelope::read_from(Path::new(&args.test_data))?;

    if envelope.clients.is_empty() {
        anyhow::bail!("Test data has no clients");
    }

    let server_pubkey = load_server_public_key_pem(&args.server_pubkey_pem)?;

    if args.target_rps > 0 {
        println!(
            "Load test (open-loop): target {} cycles/s, max_concurrent={}, {} signs/cycle",
            args.target_rps, args.max_concurrent, args.signs_per_cycle
        );
    } else {
        println!(
            "Load test (closed-loop): {} workers, {} signs/cycle",
            args.threads, args.signs_per_cycle
        );
        if args.mean_delay_ms > 0 {
            println!(
                "Mean delay: {}ms between requests per worker",
                args.mean_delay_ms
            );
        } else {
            println!("Mode: burst (no delay)");
        }
    }
    println!(
        "Test data: {} clients from {}",
        envelope.clients.len(),
        args.test_data
    );
    println!("BFF: {}", args.bff_url);

    if args.duration_secs > 0 {
        println!("Duration: {}s", args.duration_secs);
    } else {
        println!("Duration: unlimited (Ctrl+C to stop)");
    }

    let running = Arc::new(AtomicBool::new(true));
    let stats = Arc::new(Stats::new());
    let envelope = Arc::new(envelope);
    let server_pubkey = Arc::new(server_pubkey);

    // Graceful shutdown on Ctrl+C
    let running_clone = Arc::clone(&running);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\nStopping...");
        running_clone.store(false, Ordering::Relaxed);
    });

    // Duration timer
    if args.duration_secs > 0 {
        let running_clone = Arc::clone(&running);
        let duration = args.duration_secs;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(duration)).await;
            running_clone.store(false, Ordering::Relaxed);
        });
    }

    // Stats reporting timer
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

    if args.target_rps > 0 {
        open_loop(envelope, server_pubkey, args.clone(), stats.clone(), running).await;
    } else {
        closed_loop(envelope, server_pubkey, args.clone(), stats.clone(), running).await;
    }

    stats.print_report();
    Ok(())
}

/// Closed-loop: N workers each looping sequentially. Concurrency = N.
async fn closed_loop(
    envelope: Arc<TestDataEnvelope>,
    server_pubkey: Arc<josekit::jwk::Jwk>,
    args: LoadTestArgs,
    stats: Arc<Stats>,
    running: Arc<AtomicBool>,
) {
    let mut handles = Vec::with_capacity(args.threads);
    for worker_id in 0..args.threads {
        let running = Arc::clone(&running);
        let stats = Arc::clone(&stats);
        let envelope = Arc::clone(&envelope);
        let server_pubkey = Arc::clone(&server_pubkey);
        let args = args.clone();

        let handle = tokio::spawn(async move {
            worker_loop(
                worker_id,
                &envelope,
                &server_pubkey,
                &args,
                &stats,
                &running,
            )
            .await;
        });
        handles.push(handle);
    }
    for handle in handles {
        let _ = handle.await;
    }
}

/// Open-loop: one rate-paced producer fire-and-forgets each cycle as
/// its own task. Concurrency = whatever the SUT and the semaphore
/// cap allow. The producer never blocks on cycle completion, so target
/// throughput is decoupled from per-request latency.
async fn open_loop(
    envelope: Arc<TestDataEnvelope>,
    server_pubkey: Arc<josekit::jwk::Jwk>,
    args: LoadTestArgs,
    stats: Arc<Stats>,
    running: Arc<AtomicBool>,
) {
    // Build the client pool once. Share the same RestClient (and so the
    // same reqwest connection pool) across every spawned cycle.
    let rest = match RestClient::new(&args.bff_url) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            eprintln!("Failed to create REST client: {:#}", e);
            return;
        }
    };
    let clients = match build_client_pool(&envelope, &server_pubkey, &rest) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Failed to build client pool: {:#}", e);
            return;
        }
    };
    if clients.is_empty() {
        eprintln!("No clients in test data");
        return;
    }

    let semaphore = Arc::new(Semaphore::new(args.max_concurrent));
    let inter_arrival = Duration::from_secs_f64(1.0 / args.target_rps as f64);
    let mut next = Instant::now();

    while running.load(Ordering::Relaxed) {
        // Wait until the next scheduled tick. If we're already late
        // (SUT is slow and the producer can't get permits fast enough),
        // fire immediately — pacing self-corrects on the next tick.
        let now = Instant::now();
        if next > now {
            tokio::time::sleep(next - now).await;
        }
        next += inter_arrival;

        // try_acquire_owned() never awaits — preserves producer pacing
        // even when in-flight is saturated. Blocking here would couple
        // the open loop back to closed-loop semantics.
        let permit = match Arc::clone(&semaphore).try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                stats.record_saturated();
                continue;
            }
        };

        let idx = rand::rng().random_range(0..clients.len());
        let clients = Arc::clone(&clients);
        let args = args.clone();
        let stats = Arc::clone(&stats);
        let running = Arc::clone(&running);

        tokio::spawn(async move {
            // `_permit` drops on task exit, freeing the slot.
            let _permit = permit;
            let (am, pin, client_id, hsm_kid) = &clients[idx];
            if let Err(e) =
                run_one_cycle(am, pin, client_id, hsm_kid, &args, &stats, &running).await
            {
                stats.record_auth_error();
                eprintln!(
                    "cycle error for client {}...: {:#}",
                    &client_id[..12.min(client_id.len())],
                    e
                );
            }
        });
    }

    // Drain: wait for in-flight cycles to finish (best-effort).
    let _ = semaphore
        .acquire_many(args.max_concurrent as u32)
        .await;
}

fn build_client_pool(
    envelope: &TestDataEnvelope,
    server_pubkey: &josekit::jwk::Jwk,
    rest: &Arc<RestClient>,
) -> Result<ClientPool> {
    envelope
        .clients
        .iter()
        .map(|c| {
            let device_jwk =
                build_device_jwk(&c.device_key.x, &c.device_key.y, &c.device_key.d, &c.kid)?;
            let am = AccessMechanismClient::new(
                Arc::clone(rest),
                server_pubkey.clone(),
                device_jwk,
                c.kid.clone(),
                c.pin_stretch_d.clone(),
                envelope.opaque_context.clone(),
                envelope.opaque_server_identifier.clone(),
            );
            Ok((am, c.pin.clone(), c.client_id.clone(), c.hsm_kid.clone()))
        })
        .collect()
}

async fn worker_loop(
    worker_id: usize,
    envelope: &TestDataEnvelope,
    server_pubkey: &josekit::jwk::Jwk,
    args: &LoadTestArgs,
    stats: &Stats,
    running: &AtomicBool,
) {
    let rest = match RestClient::new(&args.bff_url) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            eprintln!(
                "[worker {}] Failed to create REST client: {:#}",
                worker_id, e
            );
            return;
        }
    };

    // Pre-build one AccessMechanismClient per test client — avoids JWK JSON
    // round-trips and string allocations in the hot loop.
    let clients: Vec<(AccessMechanismClient, String, String, String)> = match envelope
        .clients
        .iter()
        .map(|c| {
            let device_jwk =
                build_device_jwk(&c.device_key.x, &c.device_key.y, &c.device_key.d, &c.kid)?;
            let am = AccessMechanismClient::new(
                Arc::clone(&rest),
                server_pubkey.clone(),
                device_jwk,
                c.kid.clone(),
                c.pin_stretch_d.clone(),
                envelope.opaque_context.clone(),
                envelope.opaque_server_identifier.clone(),
            );
            Ok((am, c.pin.clone(), c.client_id.clone(), c.hsm_kid.clone()))
        })
        .collect::<Result<Vec<_>>>()
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[worker {}] Failed to build client pool: {:#}",
                worker_id, e
            );
            return;
        }
    };

    while running.load(Ordering::Relaxed) {
        let idx = rand::rng().random_range(0..clients.len());
        let (am, pin, client_id, hsm_kid) = &clients[idx];

        match run_one_cycle(am, pin, client_id, hsm_kid, args, stats, running).await {
            Ok(()) => {}
            Err(e) => {
                stats.record_auth_error();
                eprintln!(
                    "[worker {}] cycle error for client {}...: {:#}",
                    worker_id,
                    &client_id[..12.min(client_id.len())],
                    e
                );
                // Brief back-off before retrying
                tokio::time::sleep(Duration::from_millis(500)).await;
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
    // 1. OPAQUE login (create session)
    poisson_delay(args.mean_delay_ms).await;
    let t0 = Instant::now();
    let (session_key, session_id) = am.create_session(pin, client_id).await?;
    stats.record_latency(t0.elapsed().as_millis() as u64);
    stats.record_auth_cycle();

    // 2. HSM sign operations
    for _ in 0..args.signs_per_cycle {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        poisson_delay(args.mean_delay_ms).await;

        let message: [u8; 32] = rand::rng().random();
        let t_sign = Instant::now();
        match am
            .hsm_sign(&session_key, &session_id, client_id, hsm_kid, &message)
            .await
        {
            Ok(_) => {
                stats.record_latency(t_sign.elapsed().as_millis() as u64);
            }
            Err(e) => {
                stats.record_error();
                eprintln!("HSM sign error: {:#}", e);
            }
        }
    }

    Ok(())
}

/// Sleep for an exponentially distributed duration with the given mean.
/// This produces Poisson-distributed arrivals when applied before each request.
///
/// If mean_ms == 0, returns immediately (burst mode).
/// The delay is clamped to 5x the mean to avoid extreme outliers.
async fn poisson_delay(mean_ms: u64) {
    if mean_ms == 0 {
        return;
    }

    let mean = mean_ms as f64;
    // -mean * ln(1 - U) where U ~ Uniform(0,1)
    let u: f64 = rand::rng().random();
    let raw = -mean * (1.0 - u).ln();
    let clamped = raw.min(5.0 * mean);

    if clamped > 0.0 {
        tokio::time::sleep(Duration::from_millis(clamped as u64)).await;
    }
}
