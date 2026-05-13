// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Clock` — explicit-time seam for the pure functional core.
//!
//! Phase 1 / Task 1.1 of
//! [`_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`].
//! Today `vauchi-core` reads `std::time::SystemTime::now()` from 130
//! sites scattered across the storage, exchange, sync, and network
//! subsystems. That ambient reach makes the core impossible to model
//! as a function of `(state, input)` — the same state machine
//! produces different outputs on every test run.
//!
//! This module introduces the `Clock` trait. `SystemClock` calls the
//! real OS clock; `FakeClock` returns a value the test controls. Every
//! subsequent retirement MR (Phase 1 / Task 1.1 / Step 3 — cluster-by-
//! cluster) replaces a `SystemTime::now()` callsite with
//! `clock.now()`, threaded through `Vauchi` / `AppEngine`.
//!
//! Symmetric with [`rng`] (Task 1.2) — same shape, same end-state.

use std::sync::Arc;
use std::time::SystemTime;

/// An explicit-time seam. Every `vauchi-core` site that today reads
/// `SystemTime::now()` will, over Phase 1 / Task 1.1 / Step 3, route
/// through a `dyn Clock` instead.
///
/// Implementations must be cheap (`now()` is called in hot loops),
/// `Send + Sync` (the core spans threads), and monotonic *only* to the
/// extent the OS clock is — the core treats wall-clock as a source of
/// timestamps, not as an ordering primitive.
pub trait Clock: Send + Sync {
    /// Current wall-clock time.
    fn now(&self) -> SystemTime;

    /// Current Unix epoch seconds. Default impl derived from `now()`
    /// — implementors override only when a cheaper path exists.
    fn unix_seconds(&self) -> u64 {
        self.now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Production clock. Reads the OS wall-clock via `SystemTime::now()`.
///
/// Constructed via `SystemClock::new()` or `SystemClock::shared()`.
/// The `shared()` constructor returns a long-lived `Arc<dyn Clock>` so
/// every consumer in the same process shares one allocation.
#[derive(Default, Debug)]
pub struct SystemClock;

impl SystemClock {
    /// Construct a fresh `SystemClock`. Zero-sized; cheap.
    pub fn new() -> Self {
        Self
    }

    /// Construct an `Arc<dyn Clock>` for boring DI plumbing. The
    /// returned handle is the canonical wire-it-through-Vauchi shape.
    pub fn shared() -> Arc<dyn Clock> {
        Arc::new(Self)
    }
}

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Test-only clock with caller-controlled state.
///
/// Gated by the `testing` feature so production binaries cannot
/// accidentally substitute it. Construction:
///
/// ```ignore
/// use std::time::{SystemTime, Duration};
/// use vauchi_core::clock::{Clock, FakeClock};
///
/// let clock = FakeClock::new(SystemTime::UNIX_EPOCH);
/// assert_eq!(clock.unix_seconds(), 0);
///
/// clock.advance(Duration::from_secs(60));
/// assert_eq!(clock.unix_seconds(), 60);
///
/// clock.set(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000));
/// assert_eq!(clock.unix_seconds(), 1_700_000_000);
/// ```
///
/// All methods are interior-mutable so tests can advance the clock
/// without `mut` plumbing through the system under test.
#[cfg(any(test, feature = "testing"))]
pub struct FakeClock {
    inner: std::sync::Mutex<SystemTime>,
}

#[cfg(any(test, feature = "testing"))]
impl FakeClock {
    /// Construct a `FakeClock` pinned to `t`.
    pub fn new(t: SystemTime) -> Self {
        Self {
            inner: std::sync::Mutex::new(t),
        }
    }

    /// Return an `Arc<dyn Clock>` wrapping this clock, mirroring
    /// `SystemClock::shared()`. The underlying `FakeClock` is consumed.
    pub fn shared(self) -> Arc<dyn Clock> {
        Arc::new(self)
    }

    /// Advance the clock by `d`. Saturates at `SystemTime::MAX`.
    pub fn advance(&self, d: std::time::Duration) {
        let mut guard = self.inner.lock().expect("FakeClock mutex poisoned");
        *guard = guard.checked_add(d).unwrap_or(SystemTime::UNIX_EPOCH);
    }

    /// Set the clock to an absolute `t`. Useful for jumping to a
    /// specific test fixture timestamp.
    pub fn set(&self, t: SystemTime) {
        let mut guard = self.inner.lock().expect("FakeClock mutex poisoned");
        *guard = t;
    }
}

#[cfg(any(test, feature = "testing"))]
impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *self.inner.lock().expect("FakeClock mutex poisoned")
    }
}
// The `ambient_now_secs` transitional helper has been retired —
// every callsite migrated to a structural source via Slices 1–19
// of the pure-functional-core program (Phase 1 / Task 1.1 /
// Step 3b). Production code now routes wall-clock reads through
// the `Clock` trait (`SystemClock` for ambient, `FakeClock` for
// tests). Diagnostic code (`flame::output_path`) reads through
// `SystemClock::shared()` directly.
