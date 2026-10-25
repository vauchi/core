// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Mutation-coverage tests for `identity/mod.rs`.
//!
//! Kills missed mutants in try_set_display_name and master_seed;
//! Identity::drop's wipe is killed by tests/wipe_on_drop_tests.rs.

use vauchi_core::Identity;

// ============================================================
// try_set_display_name
// ============================================================

// @internal
#[test]
fn try_set_display_name_valid_name_succeeds() {
    let mut identity = Identity::create("Alice", 0);
    identity.try_set_display_name("Bob").unwrap();
    assert_eq!(identity.display_name(), "Bob");
}

// @internal
#[test]
fn try_set_display_name_empty_fails() {
    let mut identity = Identity::create("Alice", 0);
    identity
        .try_set_display_name("")
        .expect_err("empty name should fail");
    assert_eq!(
        identity.display_name(),
        "Alice",
        "name should remain unchanged after failed set"
    );
}

// @internal
#[test]
fn try_set_display_name_whitespace_only_fails() {
    let mut identity = Identity::create("Alice", 0);
    identity
        .try_set_display_name("   ")
        .expect_err("whitespace-only name should fail after normalization");
}

// ============================================================
// master_seed — returns correct deterministic value
// ============================================================

// @internal
#[test]
fn master_seed_returns_correct_value() {
    let seed = [0x42u8; 32];
    let identity = Identity::from_device_link(seed, "Alice".to_string(), 0, "Dev".to_string(), 0);
    assert_eq!(
        identity.master_seed(),
        &seed,
        "master_seed() must return the original seed"
    );
}

// @internal
#[test]
fn master_seed_differs_between_identities() {
    let id1 = Identity::create("Alice", 0);
    let id2 = Identity::create("Bob", 0);
    assert_ne!(
        id1.master_seed(),
        id2.master_seed(),
        "different identities must have different seeds"
    );
}

// Identity's Drop (the seed wipe) is checked by
// tests/wipe_on_drop_tests.rs, which reads the memory after drop
// (vauchi/private#817).
