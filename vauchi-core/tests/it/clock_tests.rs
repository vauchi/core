// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `vauchi_core::clock` — the explicit-time
//! seam introduced in Phase 1 / Task 1.1 of
//! `_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`.
//!
//! All assertions exercise public API only (`Clock`, `SystemClock`,
//! `FakeClock`). FakeClock is gated behind `feature = "testing"`,
//! which is enabled in the workspace test profile.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::clock::{Clock, FakeClock, SystemClock};

// @internal
#[test]
fn system_clock_returns_current_time() {
    let c = SystemClock::new();
    let before = SystemTime::now();
    let observed = c.now();
    let after = SystemTime::now();
    // Sandwich: the observed time must fall between two real reads of
    // the underlying OS clock.
    assert!(
        observed >= before,
        "clock went backwards: {observed:?} < {before:?}"
    );
    assert!(
        observed <= after,
        "clock raced ahead: {observed:?} > {after:?}"
    );
}

// @internal
#[test]
fn fake_clock_returns_initial_value() {
    let c = FakeClock::new(SystemTime::UNIX_EPOCH);
    assert_eq!(c.now(), SystemTime::UNIX_EPOCH);
    assert_eq!(c.unix_seconds(), 0);
}

// @internal
#[test]
fn fake_clock_advances_monotonically() {
    let c = FakeClock::new(SystemTime::UNIX_EPOCH);
    c.advance(Duration::from_secs(42));
    assert_eq!(c.unix_seconds(), 42);
    c.advance(Duration::from_secs(58));
    assert_eq!(c.unix_seconds(), 100);
}

// @internal
#[test]
fn fake_clock_set_overwrites() {
    let c = FakeClock::new(SystemTime::UNIX_EPOCH);
    let target = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    c.set(target);
    assert_eq!(c.now(), target);
    assert_eq!(c.unix_seconds(), 1_700_000_000);
}

// @internal
#[test]
fn shared_constructors_return_dyn_clock_handles() {
    // Two things matter here: the constructors fit the `Arc<dyn Clock>`
    // shape (a compile-time check) AND trait dispatch reaches the
    // correct impl through the dyn pointer (runtime). The FakeClock
    // arm gives a deterministic value we can pin.
    let sys: Arc<dyn Clock> = SystemClock::shared();
    assert!(
        sys.now() >= SystemTime::UNIX_EPOCH,
        "SystemClock via Arc<dyn Clock> returned pre-epoch time"
    );

    let fake: Arc<dyn Clock> = FakeClock::new(SystemTime::UNIX_EPOCH).shared();
    assert_eq!(
        fake.now(),
        SystemTime::UNIX_EPOCH,
        "FakeClock dispatch through Arc<dyn Clock> did not reach the fake impl"
    );
}

// @internal
#[test]
fn unix_seconds_default_impl_clamps_pre_epoch() {
    // SystemTime::UNIX_EPOCH - 1s → duration_since() returns Err.
    // The default `unix_seconds` impl swallows that and returns 0,
    // which is the documented contract; this test pins it.
    let pre_epoch = SystemTime::UNIX_EPOCH - Duration::from_secs(1);
    let c = FakeClock::new(pre_epoch);
    assert_eq!(c.unix_seconds(), 0);
}

// @internal
#[test]
fn vauchi_new_with_injects_caller_clock() {
    // Phase 1 / Task 1.1 / Step 3 wiring test: `Vauchi::new_with` must
    // store the caller-provided clock and expose it via `Vauchi::clock`.
    // This is the contract every subsequent callsite-migration MR
    // depends on — without it, `self.clock` would silently fall back
    // to SystemClock and tests would observe wall-clock time.
    use vauchi_core::{Vauchi, VauchiConfig};

    let tmp = tempfile::tempdir().expect("tempdir");
    let config = VauchiConfig::with_storage_path(tmp.path().join("vauchi.db"));
    let injected: Arc<dyn Clock> = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let vauchi = Vauchi::new_with(
        config,
        Arc::clone(&injected),
        vauchi_core::rng::OsSecureRng::shared(),
        None,
    )
    .expect("Vauchi::new_with should accept the injected clock");

    // Pointer-equality: the Arc the caller passed in is *the same* Arc
    // Vauchi now holds. Catches a future regression where someone
    // mid-MR clones-and-replaces the clock during construction (e.g.
    // wraps it in a counter-clock decorator) — a different Arc would
    // silently break testability.
    assert!(
        Arc::ptr_eq(vauchi.clock(), &injected),
        "Vauchi::new_with cloned the clock instead of storing the original Arc"
    );

    // Behavioural check: `clock().now()` returns the fixed time.
    assert_eq!(
        vauchi.clock().unix_seconds(),
        1_700_000_000,
        "Vauchi::clock() does not dispatch to the injected FakeClock"
    );
}

// @internal
#[test]
fn vauchi_default_ctor_uses_system_clock() {
    // The public default constructor must keep working with no clock
    // argument and must hand out a clock that returns a sane present-
    // day timestamp (sandwich check, mirrors `system_clock_returns_current_time`).
    use vauchi_core::{Vauchi, VauchiConfig};

    let tmp = tempfile::tempdir().expect("tempdir");
    let config = VauchiConfig::with_storage_path(tmp.path().join("vauchi.db"));
    let before = SystemTime::now();
    let vauchi = Vauchi::new(config).expect("Vauchi::new default ctor");
    let observed = vauchi.clock().now();
    let after = SystemTime::now();
    assert!(
        observed >= before && observed <= after,
        "default Vauchi clock returned {observed:?} outside [{before:?}, {after:?}]"
    );
}

// @internal
#[test]
fn unix_millis_reports_millisecond_granularity() {
    // Sub-second resolution: seconds floors to 1, millis keeps 1500.
    let c = FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_millis(1_500));
    assert_eq!(c.unix_seconds(), 1, "unix_seconds floors to whole seconds");
    assert_eq!(
        c.unix_millis(),
        1_500,
        "unix_millis keeps sub-second resolution"
    );

    // A 250 ms advance is invisible to unix_seconds but visible to
    // unix_millis — the property the multi-stage poll cadence relies on
    // (2026-06-03-multistage-qr-exchange-stalls-init-on-device): feeding
    // seconds into a millisecond frame gate froze the QR for ~1000× its
    // window.
    c.advance(Duration::from_millis(250));
    assert_eq!(
        c.unix_seconds(),
        1,
        "250ms advance does not cross a second boundary"
    );
    assert_eq!(
        c.unix_millis(),
        1_750,
        "250ms advance is visible in unix_millis"
    );
}

// @internal
#[test]
fn system_clock_unix_millis_is_monotonic_with_now() {
    // unix_millis must sit within a sandwich of real OS reads, in ms.
    let c = SystemClock::new();
    let before = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let observed = c.unix_millis();
    let after = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(
        observed >= before && observed <= after,
        "unix_millis {observed} outside [{before}, {after}]"
    );
}
