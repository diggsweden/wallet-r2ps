// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::self_test_spi_port::{CheckResult, Outcome, SelfTestProbe, Trigger};

pub struct SelfTestService {
    probes: Vec<Box<dyn SelfTestProbe>>,
}

impl SelfTestService {
    pub fn new(probes: Vec<Box<dyn SelfTestProbe>>) -> Self {
        Self { probes }
    }

    pub fn run_suite(&self, _trigger: Trigger) -> Vec<CheckResult> {
        self.probes
            .iter()
            .map(|probe| CheckResult {
                name: probe.name(),
                claim: probe.claim(),
                outcome: match probe.probe() {
                    Ok(()) => Outcome::Pass,
                    Err(e) => Outcome::Fail(e),
                },
            })
            .collect()
    }
}
