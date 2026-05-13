// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `vauchi_core::rng` — the explicit-randomness
//! seam introduced in Phase 1 / Task 1.2 of
//! `_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`.
//!
//! All assertions exercise public API only (`SecureRng`, `OsSecureRng`,
//! `DeterministicRng`). `DeterministicRng` is gated behind
//! `feature = "testing"`, enabled by the workspace test profile.

use vauchi_core::rng::{DeterministicRng, OsSecureRng, SecureRng};

// @internal
#[test]
fn os_secure_rng_yields_non_zero_bytes() {
    // Statistical sanity: 32 bytes from OsRng should not all be zero.
    // (Probability of all-zero output: 2^-256.)
    let rng = OsSecureRng::new();
    let mut buf = [0u8; 32];
    rng.fill_bytes(&mut buf);
    assert!(
        buf.iter().any(|b| *b != 0),
        "OsRng returned 32 zero bytes — implausible without a broken impl"
    );
}

// @internal
#[test]
fn deterministic_rng_reproduces_sequence() {
    let rng_a = DeterministicRng::from_seed(0xC0FFEE);
    let rng_b = DeterministicRng::from_seed(0xC0FFEE);
    for _ in 0..16 {
        assert_eq!(rng_a.random_u64(), rng_b.random_u64());
    }
}

// @internal
#[test]
fn deterministic_rng_diverges_for_different_seeds() {
    let rng_a = DeterministicRng::from_seed(1);
    let rng_b = DeterministicRng::from_seed(2);
    assert_ne!(rng_a.random_u64(), rng_b.random_u64());
}

// @internal
#[test]
fn deterministic_rng_fill_bytes_is_deterministic() {
    let rng_a = DeterministicRng::from_seed(0xDEADBEEF);
    let rng_b = DeterministicRng::from_seed(0xDEADBEEF);
    let mut buf_a = [0u8; 64];
    let mut buf_b = [0u8; 64];
    rng_a.fill_bytes(&mut buf_a);
    rng_b.fill_bytes(&mut buf_b);
    assert_eq!(buf_a, buf_b);
}

// @internal
#[test]
fn random_in_range_respects_bounds() {
    let rng = DeterministicRng::from_seed(7);
    for _ in 0..256 {
        let v = rng.random_in_range_u64(10, 20);
        assert!((10..=20).contains(&v), "value {v} outside [10, 20]");
    }
}

// @internal
#[test]
fn random_in_range_collapses_when_min_equals_max() {
    let rng = DeterministicRng::from_seed(0);
    assert_eq!(rng.random_in_range_u64(42, 42), 42);
}

// @internal
#[test]
fn random_index_respects_bound() {
    let rng = DeterministicRng::from_seed(99);
    for _ in 0..256 {
        assert!(rng.random_index(7) < 7);
    }
}

// @internal
#[test]
fn random_index_collapses_when_len_one() {
    let rng = DeterministicRng::from_seed(0);
    assert_eq!(rng.random_index(1), 0);
}

// @internal
#[test]
fn os_secure_rng_shared_returns_distinct_entropy() {
    // Smoke test: shared() returns Arc<dyn SecureRng>; sequential
    // fills draw fresh entropy from the OS CSPRNG.
    let a = OsSecureRng::shared();
    let b = OsSecureRng::shared();
    let mut ba = [0u8; 8];
    let mut bb = [0u8; 8];
    a.fill_bytes(&mut ba);
    b.fill_bytes(&mut bb);
    // Probability of accidental coincidence: 2^-64.
    assert_ne!(ba, bb);
}
