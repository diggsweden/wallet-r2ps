// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Instant;

use crate::application::clock_port::Clock;

/// Real-time `Clock` adapter used in production; `PeriodicScheduler` is tested against a fake
/// instead.
#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
