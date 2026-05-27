// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `vauchi_core::monotonic` — the explicit-
//! monotonic-time seam introduced in Phase 1 / Task 1.1b of
//! `_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`.
//!
//! All assertions exercise public API only (`MonotonicClock`,
//! `SystemMonotonicClock`, `FakeMonotonicClock`). FakeMonotonicClock is
//! gated behind `feature = "testing"`, enabled in the workspace test
//! profile.

use std::sync::Arc;
use std::time::Duration;

use vauchi_core::monotonic::{FakeMonotonicClock, MonotonicClock, SystemMonotonicClock};

// @internal
#[test]
fn system_clock_never_goes_backwards() {
    let c = SystemMonotonicClock::new();
    let t0 = c.now();
    let t1 = c.now();
    assert!(t1 >= t0, "monotonic clock went backwards: {t1:?} < {t0:?}");
}

// @internal
#[test]
fn fake_clock_does_not_advance_without_advance_call() {
    let c = FakeMonotonicClock::new();
    let t0 = c.now();
    let t1 = c.now();
    assert_eq!(
        t1.duration_since(t0),
        Duration::ZERO,
        "fake clock advanced without an explicit advance()"
    );
}

// @internal
#[test]
fn fake_clock_advance_is_exact_and_relative() {
    let c = FakeMonotonicClock::new();
    let t0 = c.now();
    c.advance(Duration::from_secs(30));
    let t1 = c.now();
    assert_eq!(t1.duration_since(t0), Duration::from_secs(30));

    c.advance(Duration::from_millis(500));
    let t2 = c.now();
    assert_eq!(t2.duration_since(t0), Duration::from_millis(30_500));
    assert_eq!(t2.duration_since(t1), Duration::from_millis(500));
}

// @internal
#[test]
fn fake_clock_advance_accumulates() {
    let c = FakeMonotonicClock::new();
    let t0 = c.now();
    for _ in 0..10 {
        c.advance(Duration::from_secs(1));
    }
    assert_eq!(c.now().duration_since(t0), Duration::from_secs(10));
}

// @internal
#[test]
fn shared_constructors_return_dyn_handles() {
    // Compile-time: constructors fit `Arc<dyn MonotonicClock>`. Runtime:
    // dispatch reaches the correct impl. The fake arm pins a value.
    let sys: Arc<dyn MonotonicClock> = SystemMonotonicClock::shared();
    let s0 = sys.now();
    assert!(
        sys.now() >= s0,
        "SystemMonotonicClock via Arc went backwards"
    );

    // Canonical injection pattern: keep the concrete Arc to drive
    // `advance()`, hand a cloned `dyn` handle to the system under test.
    // Both share one inner offset via the Arc.
    let fake = Arc::new(FakeMonotonicClock::new());
    let injected: Arc<dyn MonotonicClock> = fake.clone();
    let f0 = injected.now();
    fake.advance(Duration::from_secs(7));
    assert_eq!(
        injected.now().duration_since(f0),
        Duration::from_secs(7),
        "advance() on the concrete Arc was not visible through the dyn handle"
    );
}

// @internal
#[test]
fn vauchi_with_monotonic_injects_caller_clock() {
    // Wiring contract every callsite-migration MR depends on:
    // `Vauchi::with_monotonic` must store the caller-provided clock and
    // expose it via `Vauchi::monotonic`. Without it, `self.monotonic`
    // silently falls back to the real OS clock and timeout tests would
    // block on wall-clock.
    use vauchi_core::Vauchi;

    let injected: Arc<dyn MonotonicClock> = Arc::new(FakeMonotonicClock::new());
    let vauchi = Vauchi::in_memory()
        .expect("in_memory Vauchi")
        .with_monotonic(Arc::clone(&injected));

    assert!(
        Arc::ptr_eq(vauchi.monotonic(), &injected),
        "with_monotonic cloned the clock instead of storing the original Arc"
    );
}

// @internal
#[test]
fn vauchi_default_ctor_uses_system_monotonic_clock() {
    // The default constructor must hand out a working monotonic clock
    // with no injection — a sandwich check that it never goes backwards.
    use vauchi_core::Vauchi;

    let vauchi = Vauchi::in_memory().expect("in_memory Vauchi");
    let t0 = vauchi.monotonic().now();
    let t1 = vauchi.monotonic().now();
    assert!(
        t1 >= t0,
        "default Vauchi monotonic clock went backwards: {t1:?} < {t0:?}"
    );
}
