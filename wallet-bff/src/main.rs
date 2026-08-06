// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use wallet_bff::infrastructure::telemetry;

#[tokio::main]
async fn main() {
    let _telemetry = telemetry::init("wallet-bff").expect("failed to init telemetry");

    wallet_bff::run().await;
}
