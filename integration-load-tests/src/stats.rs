// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Stats tracking and reporting for load tests.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub struct Stats {
    latencies: Mutex<Vec<u64>>,
    errors: AtomicU64,
    auth_cycles: AtomicU64,
    auth_errors: AtomicU64,
    start_time: Instant,
}

pub struct StatsSnapshot {
    pub total_requests: u64,
    pub total_errors: u64,
    pub total_auth_cycles: u64,
    pub total_auth_errors: u64,
    pub avg_latency_ms: u64,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub max_latency_ms: u64,
    pub requests_per_second: f64,
    pub elapsed_seconds: u64,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            latencies: Mutex::new(Vec::new()),
            errors: AtomicU64::new(0),
            auth_cycles: AtomicU64::new(0),
            auth_errors: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    pub fn record_latency(&self, ms: u64) {
        self.latencies.lock().unwrap().push(ms);
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_auth_cycle(&self) {
        self.auth_cycles.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_auth_error(&self) {
        self.auth_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let elapsed = self.start_time.elapsed().as_secs();
        let mut sorted = self.latencies.lock().unwrap().clone();
        sorted.sort_unstable();
        let total = sorted.len() as u64;

        let avg = sorted.iter().sum::<u64>().checked_div(total).unwrap_or(0);
        let p50 = percentile(&sorted, 0.50);
        let p95 = percentile(&sorted, 0.95);
        let p99 = percentile(&sorted, 0.99);
        let max = sorted.last().copied().unwrap_or(0);

        let rps = if elapsed > 0 {
            total as f64 / elapsed as f64
        } else {
            0.0
        };

        StatsSnapshot {
            total_requests: total,
            total_errors: self.errors.load(Ordering::Relaxed),
            total_auth_cycles: self.auth_cycles.load(Ordering::Relaxed),
            total_auth_errors: self.auth_errors.load(Ordering::Relaxed),
            avg_latency_ms: avg,
            p50_latency_ms: p50,
            p95_latency_ms: p95,
            p99_latency_ms: p99,
            max_latency_ms: max,
            requests_per_second: rps,
            elapsed_seconds: elapsed,
        }
    }

    /// Print a stats summary line to stdout.
    pub fn print_summary(&self) {
        let s = self.snapshot();
        println!(
            "[{}s] reqs={} err={} auth={} auth_err={} rps={:.2} avg={}ms p50={}ms p95={}ms p99={}ms max={}ms",
            s.elapsed_seconds,
            s.total_requests,
            s.total_errors,
            s.total_auth_cycles,
            s.total_auth_errors,
            s.requests_per_second,
            s.avg_latency_ms,
            s.p50_latency_ms,
            s.p95_latency_ms,
            s.p99_latency_ms,
            s.max_latency_ms,
        );
    }

    /// Print a final detailed report.
    pub fn print_report(&self) {
        let s = self.snapshot();
        println!();
        println!("--- Load Test Report ---");
        println!("Duration:      {}s", s.elapsed_seconds);
        println!("Total reqs:    {}", s.total_requests);
        println!("Total errors:  {}", s.total_errors);
        println!("Auth cycles:   {}", s.total_auth_cycles);
        println!("Auth errors:   {}", s.total_auth_errors);
        println!("Throughput:    {:.2} req/s", s.requests_per_second);
        println!("Avg latency:   {}ms", s.avg_latency_ms);
        println!("p50 latency:   {}ms", s.p50_latency_ms);
        println!("p95 latency:   {}ms", s.p95_latency_ms);
        println!("p99 latency:   {}ms", s.p99_latency_ms);
        println!("Max latency:   {}ms", s.max_latency_ms);
        println!("------------------------");
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64) * p) as usize;
    sorted[idx.min(sorted.len() - 1)]
}
