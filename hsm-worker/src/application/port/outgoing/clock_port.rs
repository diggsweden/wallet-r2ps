// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use chrono::{DateTime, Utc};
use std::time::Instant;

/// A source of monotonic time, injected so time-driven decisions (e.g. the periodic self-test
/// scheduler) can be unit-tested without real sleeping. Constructed and consumed within a
/// single thread (the periodic trigger's worker thread), so no `Send`/`Sync` bound is needed.
pub trait Clock {
    fn now(&self) -> Instant;
}

/// A source of wall-clock time for audit records — FAU_GEN.1.2 requires a date and time on
/// every record. Kept separate from `Clock`, which must stay monotonic: a wall-clock jump would
/// corrupt `PeriodicScheduler`'s interval arithmetic. `Send`/`Sync` is required because
/// `SelfTestService` is shared between the start-up and periodic self-test threads.
pub trait WallClock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
}
