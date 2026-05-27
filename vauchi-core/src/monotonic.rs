// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `MonotonicClock` — explicit-monotonic-time seam for the pure
//! functional core.
//!
//! Phase 1 / Task 1.1b of
//! [`_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`].
//! Sibling of [`clock`] (wall-clock `SystemTime`), [`rng`], and
//! [`sleeper`]. Where `Clock` supplies *timestamps*, this seam supplies
//! *monotonic durations*: the `std::time::Instant` reads that drive
//! session timeouts, retry backoff windows, and deadline checks inside
//! `vauchi-core`'s state machines. Those reads make the same state
//! machine produce a different transition on every run, so the core is
//! not a function of `(state, input)` until they route through a seam.
//!
//! **Scope (decided 2026-05-27):** only the determinism-relevant
//! callsites — timeouts, deadlines, retry windows inside state machines
//! — migrate here. Diagnostic / perf-measurement `Instant::now()` reads
//! (e.g. `qr/scanner.rs` decode timing, `diagnostic/*`, transport
//! traces, `audio_cpal` capture timing) are *exempt*: they measure
//! elapsed wall-time for logs and never feed a state transition, so
//! they do not break `(state, input) → output` determinism. This
//! mirrors the `OsRng` carve-out for crypto RNG.
//!
//! ## Why a fresh type instead of reusing `Instant` directly
//!
//! `Instant` is opaque — there is no public constructor for an
//! arbitrary value, so a `FakeClock`-style implementation cannot *pin*
//! it to a fixture the way [`clock::FakeClock`] pins `SystemTime`.
//! [`FakeMonotonicClock`] sidesteps this by capturing one real
//! `Instant` at construction and returning `start + offset`; tests
//! advance `offset`. Every comparison the core performs is *relative*
//! (`now.duration_since(earlier)`, `now >= deadline`), so a controllable
//! offset is sufficient — and the public type stays `Instant`, so no
//! stored field's type changes.
//!
//! **Migration note:** `Instant::elapsed()` calls the real OS clock
//! internally (`Instant::now() - self`), bypassing this seam. A site
//! that migrates must replace `earlier.elapsed()` with
//! `monotonic.now().duration_since(earlier)` so both the set-site and
//! the compare-site read the same clock.

use std::sync::Arc;
use std::time::Instant;

/// An explicit-monotonic-time seam. Every `vauchi-core` state-machine
/// site that today reads `std::time::Instant::now()` for a timeout,
/// deadline, or retry window routes through a `dyn MonotonicClock`
/// instead.
///
/// Implementations must be cheap (`now()` is polled in timeout loops)
/// and `Send + Sync` (the core spans threads). The returned `Instant`
/// is meaningful only *relative* to other `Instant`s from the same
/// clock — never compared against a bare `Instant::now()`.
pub trait MonotonicClock: Send + Sync {
    /// Current monotonic instant. Only relative comparisons against
    /// other values from the *same* clock are meaningful.
    fn now(&self) -> Instant;
}

/// Production monotonic clock. Reads the OS monotonic clock via
/// `Instant::now()`.
///
/// Constructed via `SystemMonotonicClock::new()` or
/// `SystemMonotonicClock::shared()`. The `shared()` constructor returns
/// a long-lived `Arc<dyn MonotonicClock>` so every consumer in the same
/// process shares one allocation, matching [`clock::SystemClock`].
#[derive(Default, Debug)]
pub struct SystemMonotonicClock;

impl SystemMonotonicClock {
    /// Construct a fresh `SystemMonotonicClock`. Zero-sized; cheap.
    pub fn new() -> Self {
        Self
    }

    /// Construct an `Arc<dyn MonotonicClock>` for boring DI plumbing.
    pub fn shared() -> Arc<dyn MonotonicClock> {
        Arc::new(Self)
    }
}

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Test-only monotonic clock with caller-controlled advance.
///
/// Gated by the `testing` feature so production binaries cannot
/// accidentally substitute it. Captures one real `Instant` at
/// construction (`start`) and returns `start + offset`; tests move
/// `offset` forward with [`advance`](FakeMonotonicClock::advance).
///
/// ```ignore
/// use std::time::Duration;
/// use vauchi_core::monotonic::{FakeMonotonicClock, MonotonicClock};
///
/// let clock = FakeMonotonicClock::new();
/// let t0 = clock.now();
/// clock.advance(Duration::from_secs(30));
/// let t1 = clock.now();
/// assert_eq!(t1.duration_since(t0), Duration::from_secs(30));
/// ```
///
/// All methods are interior-mutable so tests can advance the clock
/// without `mut` plumbing through the system under test.
#[cfg(any(test, feature = "testing"))]
pub struct FakeMonotonicClock {
    start: Instant,
    offset: std::sync::Mutex<std::time::Duration>,
}

#[cfg(any(test, feature = "testing"))]
impl FakeMonotonicClock {
    /// Construct a `FakeMonotonicClock` at offset zero.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            offset: std::sync::Mutex::new(std::time::Duration::ZERO),
        }
    }

    /// Return an `Arc<dyn MonotonicClock>` wrapping this clock,
    /// mirroring `SystemMonotonicClock::shared()`.
    pub fn shared(self) -> Arc<dyn MonotonicClock> {
        Arc::new(self)
    }

    /// Advance the clock by `d`. Saturates at the maximum representable
    /// offset.
    pub fn advance(&self, d: std::time::Duration) {
        let mut guard = self
            .offset
            .lock()
            .expect("FakeMonotonicClock mutex poisoned");
        *guard = guard.saturating_add(d);
    }
}

#[cfg(any(test, feature = "testing"))]
impl Default for FakeMonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "testing"))]
impl MonotonicClock for FakeMonotonicClock {
    fn now(&self) -> Instant {
        let offset = *self
            .offset
            .lock()
            .expect("FakeMonotonicClock mutex poisoned");
        self.start
            .checked_add(offset)
            .expect("FakeMonotonicClock offset overflow")
    }
}
