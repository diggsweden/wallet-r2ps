// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::{Duration, Instant};

use crate::application::clock_port::Clock;

/// Decides when a periodic self-test run is due (FPT_TST.1.1). Pure aside from the injected
/// `Clock`, so the decision is unit-testable without real sleeping — the actual thread loop
/// lives in the `PeriodicSelfTestTrigger` infrastructure adapter.
pub struct PeriodicScheduler<C: Clock> {
    clock: C,
    interval: Duration,
    last_run: Instant,
}

impl<C: Clock> PeriodicScheduler<C> {
    pub fn new(clock: C, interval: Duration) -> Self {
        let last_run = clock.now();
        Self {
            clock,
            interval,
            last_run,
        }
    }

    /// True at most once per interval. Resets the internal clock when it fires, so a caller
    /// that misses several intervals (e.g. the process was stalled) gets a single run rather
    /// than a catch-up burst.
    pub fn due(&mut self) -> bool {
        let now = self.clock.now();
        if now.duration_since(self.last_run) >= self.interval {
            self.last_run = now;
            true
        } else {
            false
        }
    }
}
