// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use crate::application::clock_port::{Clock, WallClock};
use chrono::{DateTime, Utc};
use std::time::Instant;

/// Real-time `Clock` and `WallClock` adapter used in production; `PeriodicScheduler` and
/// `SelfTestService` are tested against fakes instead.
#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl WallClock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
