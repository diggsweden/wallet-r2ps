// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use hsm_worker::infrastructure::telemetry;
use hsm_worker::run;
use tracing::instrument;

#[instrument(name = "main", skip_all)]
fn main() {
    dotenvy::dotenv().ok();

    // Tokio runtime is required by the OTLP batch span exporter (grpc-tonic).
    // hsm-worker itself is sync, so we only need the runtime context to be
    // active for the duration of the program.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .expect("failed to build tokio runtime");
    let _runtime_guard = runtime.enter();

    let _telemetry = telemetry::init("hsm-worker").expect("failed to init telemetry");

    run();
}
