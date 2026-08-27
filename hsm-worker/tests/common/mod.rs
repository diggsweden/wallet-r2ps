// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Shared fixtures for the `tests/` integration binaries.

use hsm_worker::application::self_test_spi_port::CheckResult;
use hsm_worker::application::self_test_spi_port::Outcome::Pass;
use hsm_worker::application::self_test_spi_port::TsfClaim::CryptographicLibraries;
use hsm_worker::application::service::TsfHealth;

/// A `TsfHealth` that has already observed a passing suite result.
pub fn healthy() -> TsfHealth {
    let health = TsfHealth::new();
    health.apply(&[CheckResult {
        name: "test",
        claim: CryptographicLibraries,
        outcome: Pass,
    }]);
    health
}
