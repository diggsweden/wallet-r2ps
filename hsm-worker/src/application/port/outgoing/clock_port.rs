// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Instant;

/// A source of monotonic time, injected so time-driven decisions (e.g. the periodic self-test
/// scheduler) can be unit-tested without real sleeping. Constructed and consumed within a
/// single thread (the periodic trigger's worker thread), so no `Send`/`Sync` bound is needed.
pub trait Clock {
    fn now(&self) -> Instant;
}
