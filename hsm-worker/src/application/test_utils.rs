// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::{
    self_test_spi_port::{CheckResult, Outcome::Pass, TsfClaim::CryptographicLibraries},
    service::TsfHealth,
};

pub fn healthy() -> TsfHealth {
    let health = TsfHealth::new();
    health.apply(&[CheckResult {
        name: "test_fixture",
        claim: CryptographicLibraries,
        outcome: Pass,
    }]);
    health
}
