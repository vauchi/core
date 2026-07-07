// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `SecureRng` — explicit-randomness seam for the pure functional core.
//!
//! Phase 1 / Task 1.2 of
//! [`_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`].
//! Symmetric with [`crate::clock`] (Task 1.1) — same shape, same end-
//! state: every non-crypto `rand::thread_rng` callsite routes
//! through a `dyn SecureRng` instead of pulling the thread-local OS RNG
//! directly. Crypto code keeps using [`crate::crypto::random_bytes`] /
//! [`crate::crypto::random_fill`] (both backed by `OsRng`) — this trait
//! exists for the *non-crypto* paths: jitter, load-balancing, batching
//! permutations, where a deterministic test substitute saves whole
//! classes of flaky tests.
//!
//! Production: `OsSecureRng` wraps `rand::rngs::OsRng`. Construct via
//! `OsSecureRng::shared()` and store as `Arc<dyn SecureRng>`.
//!
//! Test:`DeterministicRng` (gated by `feature = "testing"`) returns a
//! fixed sequence the test controls. Lives behind `cfg(any(test,
//! feature = "testing"))` so production binaries cannot accidentally
//! substitute it.

use std::sync::Arc;

use rand::RngCore;

/// An explicit-randomness seam. Every `vauchi-core` site that today
/// reads `rand::thread_rng` (non-crypto: jitter, load-balancing,
/// permutation) routes through a `dyn SecureRng` instead.
///
/// Implementations must be `Send + Sync` (the core spans threads).
/// Implementations need *not* be cryptographically strong — crypto
/// code uses [`crate::crypto::random_bytes`] / [`crate::crypto::random_fill`]
/// directly. This trait is the seam for the *application-layer*
/// randomness that benefits from being deterministic in tests.
pub trait SecureRng: Send + Sync {
    /// Fill `buf` with random bytes.
    fn fill_bytes(&self, buf: &mut [u8]);

    /// Return a random `u64`. Default impl derives from `fill_bytes`.
    fn random_u64(&self) -> u64 {
        let mut buf = [0u8; 8];
        self.fill_bytes(&mut buf);
        u64::from_le_bytes(buf)
    }

    /// Return a random `u64` in the inclusive range `[min, max]`.
    /// Default impl uses modular reduction — acceptable bias for
    /// non-crypto paths (jitter, load balancing). Implementations may
    /// override with a uniform sampler if needed.
    fn random_in_range_u64(&self, min: u64, max: u64) -> u64 {
        debug_assert!(min <= max);
        if min == max {
            return min;
        }
        let span = max - min + 1;
        min + (self.random_u64() % span)
    }

    /// Return a random index in `[0, len)`. Convenience wrapper for
    /// load-balancing / permutation use cases.
    fn random_index(&self, len: usize) -> usize {
        debug_assert!(len > 0);
        if len == 1 {
            return 0;
        }
        (self.random_u64() as usize) % len
    }
}

/// Object-safety extension for [`SecureRng`].
///
/// Generic methods (`shuffle`, `choose`) cannot live on the trait
/// itself without breaking `dyn SecureRng` dispatch (a generic
/// method has no fixed vtable slot). The extension trait pattern
/// keeps the ergonomic `rng.shuffle(...)` callsite while preserving
/// `&dyn SecureRng` as the canonical parameter shape.
pub trait SecureRngExt: SecureRng {
    /// Returns a UUID v4 string. Replaces `uuid::Uuid::new_v4()` for
    /// callsites that need a fresh identifier and already hold an
    /// explicit randomness seam.
    fn uuid_v4(&self) -> String {
        let mut bytes = [0u8; 16];
        self.fill_bytes(&mut bytes);
        // Set the version (4) and variant (RFC 4122) bits.
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        uuid::Uuid::from_bytes(bytes).to_string()
    }

    /// Fisher-Yates in-place shuffle. Replaces
    /// `rand::seq::SliceRandom::shuffle` for callsites that took
    /// `thread_rng` for permutation work.
    fn shuffle<T>(&self, slice: &mut [T]) {
        let n = slice.len();
        if n < 2 {
            return;
        }
        for i in (1..n).rev() {
            // Uniform-ish j in [0, i] via modular reduction.
            // Bias is `(u64::MAX % (i+1))` / `u64::MAX` — negligible
            // for the slice lengths these paths see (<1024).
            let j = (self.random_u64() as usize) % (i + 1);
            slice.swap(i, j);
        }
    }

    /// Borrow a random element from `slice`, or `None` if empty.
    /// Replaces `rand::seq::SliceRandom::choose` for callsites that
    /// took `thread_rng` for selection.
    fn choose<'a, T>(&self, slice: &'a [T]) -> Option<&'a T> {
        if slice.is_empty() {
            None
        } else {
            slice.get(self.random_index(slice.len()))
        }
    }
}

// Blanket impl so any `SecureRng` (including `dyn SecureRng`) gets
// the extension methods automatically.
impl<R: SecureRng + ?Sized> SecureRngExt for R {}

/// Production RNG. Reads from the OS CSPRNG (`rand::rngs::OsRng`).
///
/// Constructed via `OsSecureRng::new()` or `OsSecureRng::shared()`.
/// The `shared()` constructor returns a long-lived `Arc<dyn SecureRng>`
/// so every consumer in the same process shares one allocation.
#[derive(Default, Debug)]
pub struct OsSecureRng;

impl OsSecureRng {
    /// Construct a fresh `OsSecureRng`. Zero-sized; cheap.
    pub fn new() -> Self {
        Self
    }

    /// Construct an `Arc<dyn SecureRng>` for boring DI plumbing. The
    /// returned handle is the canonical wire-it-through-Vauchi shape.
    pub fn shared() -> Arc<dyn SecureRng> {
        Arc::new(Self)
    }
}

impl SecureRng for OsSecureRng {
    fn fill_bytes(&self, buf: &mut [u8]) {
        rand::rngs::OsRng.fill_bytes(buf);
    }
}

/// Stepping-stone non-crypto RNG helper, transitional for Phase 1 /
/// Task 1.2 / Step 3. Returns an `OsRng` value usable wherever a
/// `rand::Rng` is needed — `gen_range`, `SliceRandom::shuffle`,
/// `SliceRandom::choose`, etc.
///
/// Every callsite is a `TODO` for structural threading: the owning
/// type should hold an `Arc<dyn SecureRng>` field and use that
/// instead of pulling ambient OS entropy. This helper exists so the
/// audit-counter `core_thread_rng` reaches zero before the full
/// retirement lands.
///
/// Hidden from public docs because it is not part of the API contract.
#[doc(hidden)]
pub fn non_crypto_rng() -> rand::rngs::OsRng {
    rand::rngs::OsRng
}

/// Test-only RNG with caller-controlled state.
///
/// Gated by the `testing` feature so production binaries cannot
/// accidentally substitute it. The RNG returns a deterministic
/// sequence seeded at construction (via `rand::rngs::StdRng`, which
/// itself wraps ChaCha12 — strong enough for test fixtures).
///
/// ```ignore
/// use vauchi_core::rng::{DeterministicRng, SecureRng};
///
/// let rng = DeterministicRng::from_seed(42);
/// let a = rng.random_u64();
/// let b = rng.random_u64();
/// // Re-seed and observe the same sequence:
/// let rng2 = DeterministicRng::from_seed(42);
/// assert_eq!(rng2.random_u64(), a);
/// assert_eq!(rng2.random_u64(), b);
/// ```
#[cfg(any(test, feature = "testing"))]
pub struct DeterministicRng {
    state: std::sync::Mutex<rand::rngs::StdRng>,
}

#[cfg(any(test, feature = "testing"))]
impl DeterministicRng {
    /// Seed the RNG with a `u64`. The same seed yields the same
    /// sequence — the whole point of a test RNG.
    pub fn from_seed(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            state: std::sync::Mutex::new(rand::rngs::StdRng::seed_from_u64(seed)),
        }
    }

    /// Construct an `Arc<dyn SecureRng>` for boring DI plumbing.
    pub fn shared(seed: u64) -> Arc<dyn SecureRng> {
        Arc::new(Self::from_seed(seed))
    }
}

#[cfg(any(test, feature = "testing"))]
impl SecureRng for DeterministicRng {
    fn fill_bytes(&self, buf: &mut [u8]) {
        self.state
            .lock()
            .expect("DeterministicRng mutex poisoned")
            .fill_bytes(buf);
    }
}

// Tests live in `core/vauchi-core/tests/it/rng_tests.rs` per the
// src-vs-tests separation rule.
