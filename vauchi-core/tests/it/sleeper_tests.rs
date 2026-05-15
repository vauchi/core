// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `vauchi_core::sleeper` — the explicit
//! suspension seam introduced in Phase 1 / Task 1.3 of
//! `_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`.
//!
//! All assertions exercise public API only (`Sleeper`, `SystemSleeper`,
//! `FakeSleeper`). `FakeSleeper` is gated behind `feature = "testing"`,
//! which is enabled in the workspace test profile.

use std::sync::Arc;
use std::time::Duration;

use vauchi_core::sleeper::{FakeSleeper, Sleeper, SystemSleeper};

// @internal
#[test]
fn system_sleeper_with_zero_duration_is_noop() {
    // Production impl must not panic on zero duration (the
    // duress-floor calls `sleep(floor - elapsed)` which is zero
    // when the elapsed time already exceeds the floor).
    SystemSleeper::new().sleep(Duration::ZERO);
}

// @internal
#[test]
fn fake_sleeper_records_calls_in_order() {
    let fake = FakeSleeper::new();
    fake.sleep(Duration::from_millis(10));
    fake.sleep(Duration::from_millis(20));
    fake.sleep(Duration::from_millis(40));
    assert_eq!(
        fake.calls(),
        vec![
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_millis(40),
        ],
    );
}

// @internal
#[test]
fn fake_sleeper_total_sums_durations() {
    let fake = FakeSleeper::new();
    fake.sleep(Duration::from_millis(100));
    fake.sleep(Duration::from_millis(250));
    assert_eq!(fake.total(), Duration::from_millis(350));
}

// @internal
#[test]
fn fake_sleeper_starts_empty() {
    let fake = FakeSleeper::new();
    assert!(fake.calls().is_empty());
    assert_eq!(fake.total(), Duration::ZERO);
}

// @internal
#[test]
fn shared_returns_dyn_sleeper() {
    let sleeper: Arc<dyn Sleeper> = SystemSleeper::shared();
    sleeper.sleep(Duration::ZERO);
}

// @internal
#[test]
fn fake_sleeper_shared_via_arc_for_test_di_pattern() {
    // The canonical test pattern: wrap FakeSleeper in Arc, clone for
    // both the system-under-test and the assertion handle.
    let fake: Arc<FakeSleeper> = Arc::new(FakeSleeper::new());
    let sleeper: Arc<dyn Sleeper> = fake.clone();
    sleeper.sleep(Duration::from_millis(50));
    sleeper.sleep(Duration::from_millis(100));
    // The original Arc<FakeSleeper> handle still sees the recorded
    // calls (interior mutability via Mutex).
    assert_eq!(
        fake.calls(),
        vec![Duration::from_millis(50), Duration::from_millis(100)],
    );
}
