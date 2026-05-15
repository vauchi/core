// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Sleeper` — explicit-suspension seam for the pure functional core.
//!
//! Phase 1 / Task 1.3 of
//! [`_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`].
//! Today `vauchi-core` calls `std::thread::sleep` from four
//! production sites (network reconnect backoff, two audio capture
//! waits, and the duress timing-floor). That ambient suspension
//! makes the core impossible to test deterministically — tests
//! block on real wall-clock for every reconnect, every audio
//! capture, every duress-floor pad.
//!
//! This module introduces the `Sleeper` trait. `SystemSleeper`
//! calls `std::thread::sleep`; `FakeSleeper` records the requested
//! duration and returns immediately, so tests can both (a) verify
//! that the sleep was invoked with the right amount and (b) run
//! at memory speed.
//!
//! Symmetric with [`clock`] — same trait shape, same end-state.
//! Combined with Phase 1 / Task 1.1 (`Clock`) and Phase 1 / Task
//! 1.2 (`SecureRng`), the `vauchi-core` interior becomes a pure
//! function of `(state, input, clock, rng, sleeper)`.

use std::sync::Arc;
use std::time::Duration;

/// An explicit-suspension seam. Every `vauchi-core` site that today
/// reads `std::thread::sleep(d)` will route through `sleeper.sleep(d)`
/// instead.
///
/// Implementations must be `Send + Sync` (the core spans threads).
/// The trait is sync — async sleep belongs to the runtime layer,
/// not to `vauchi-core`.
pub trait Sleeper: Send + Sync {
    /// Suspend the current thread for at least `d`. Implementations
    /// must not panic on a zero duration.
    fn sleep(&self, d: Duration);
}

/// Production sleeper. Delegates to `std::thread::sleep`.
///
/// Constructed via `SystemSleeper::new()` or `SystemSleeper::shared()`.
/// The `shared()` constructor returns a long-lived `Arc<dyn Sleeper>`
/// so every consumer in the same process shares one allocation.
#[derive(Default, Debug)]
pub struct SystemSleeper;

impl SystemSleeper {
    /// Construct a fresh `SystemSleeper`. Zero-sized; cheap.
    pub fn new() -> Self {
        Self
    }

    /// Construct an `Arc<dyn Sleeper>` for boring DI plumbing. The
    /// returned handle is the canonical wire-it-through shape,
    /// matching `SystemClock::shared()`.
    pub fn shared() -> Arc<dyn Sleeper> {
        Arc::new(Self)
    }
}

impl Sleeper for SystemSleeper {
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Test-only sleeper. Records each requested duration in call order
/// and returns immediately (no real wall-clock suspension).
///
/// Gated by the `testing` feature so production binaries cannot
/// accidentally substitute it. Construction:
///
/// ```ignore
/// use std::sync::Arc;
/// use std::time::Duration;
/// use vauchi_core::sleeper::{FakeSleeper, Sleeper};
///
/// let fake = Arc::new(FakeSleeper::new());
/// fake.sleep(Duration::from_millis(100));
/// fake.sleep(Duration::from_millis(200));
/// assert_eq!(
///     fake.calls(),
///     vec![Duration::from_millis(100), Duration::from_millis(200)]
/// );
/// ```
///
/// All methods are interior-mutable so tests can inspect recorded
/// calls after the sleeper has been wrapped in `Arc<dyn Sleeper>`
/// and threaded through the system under test.
#[cfg(any(test, feature = "testing"))]
pub struct FakeSleeper {
    calls: std::sync::Mutex<Vec<Duration>>,
}

#[cfg(any(test, feature = "testing"))]
impl FakeSleeper {
    /// Construct an empty `FakeSleeper`.
    pub fn new() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Return all durations passed to `sleep`, in call order.
    pub fn calls(&self) -> Vec<Duration> {
        self.calls
            .lock()
            .expect("FakeSleeper mutex poisoned")
            .clone()
    }

    /// Return the total wall-clock time that would have been spent
    /// in real sleeps (sum of recorded durations). Useful for
    /// assertions on backoff timing semantics without depending on
    /// the exact call sequence.
    pub fn total(&self) -> Duration {
        self.calls
            .lock()
            .expect("FakeSleeper mutex poisoned")
            .iter()
            .copied()
            .sum()
    }
}

#[cfg(any(test, feature = "testing"))]
impl Default for FakeSleeper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "testing"))]
impl Sleeper for FakeSleeper {
    fn sleep(&self, d: Duration) {
        self.calls
            .lock()
            .expect("FakeSleeper mutex poisoned")
            .push(d);
    }
}
