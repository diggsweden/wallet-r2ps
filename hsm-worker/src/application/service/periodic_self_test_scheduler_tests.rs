// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::application::clock_port::Clock;
use crate::application::service::periodic_self_test_scheduler::PeriodicScheduler;

struct FakeClock {
    now: Cell<Instant>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            now: Cell::new(Instant::now()),
        }
    }

    fn advance(&self, by: Duration) {
        self.now.set(self.now.get() + by);
    }
}

impl Clock for &FakeClock {
    fn now(&self) -> Instant {
        self.now.get()
    }
}

#[test]
fn not_due_before_the_interval_elapses() {
    let clock = FakeClock::new();
    let mut scheduler = PeriodicScheduler::new(&clock, Duration::from_secs(60));

    clock.advance(Duration::from_secs(59));

    assert!(!scheduler.due());
}

#[test]
fn due_once_the_interval_elapses_and_resets() {
    let clock = FakeClock::new();
    let mut scheduler = PeriodicScheduler::new(&clock, Duration::from_secs(60));

    clock.advance(Duration::from_secs(60));
    assert!(scheduler.due());

    // Immediately re-checking without further advancing must not fire again.
    assert!(!scheduler.due());
}

#[test]
fn skipping_multiple_intervals_fires_once_not_a_catch_up_burst() {
    let clock = FakeClock::new();
    let mut scheduler = PeriodicScheduler::new(&clock, Duration::from_secs(60));

    clock.advance(Duration::from_secs(600));

    assert!(scheduler.due());
    assert!(!scheduler.due());
}
