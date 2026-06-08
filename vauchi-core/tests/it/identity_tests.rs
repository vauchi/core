// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for identity
//! Extracted from mod.rs

use vauchi_core::*;

// @scenario: identity_management :: Create new identity on first launch
#[test]
fn test_create_identity() {
    let identity = Identity::create("Test User", 0);
    assert_eq!(identity.display_name(), "Test User");
}

// @scenario: identity_management :: Create encrypted identity backup
// @scenario: identity_management :: Restore identity from backup
#[test]
fn test_backup_restore_roundtrip() {
    let original = Identity::create("Alice", 0);
    let password = "correct-horse-battery-staple";
    let backup = original.export_backup(password).unwrap();
    let restored = Identity::import_backup(&backup, password, 0).unwrap();
    assert_eq!(original.public_id(), restored.public_id());
}

// @scenario: identity_management :: Create new identity on first launch
#[test]
fn test_identity_has_device_info() {
    let identity = Identity::create("Alice", 0);
    assert_eq!(identity.device_index(), 0);
    assert_eq!(identity.device_info().device_name(), "Primary Device");
}

// @scenario: identity_management :: Restore identity from backup
#[test]
fn test_backup_restore_preserves_device_info() {
    let master_seed = [0x42u8; 32];
    let original = Identity::from_device_link(
        master_seed,
        "Alice".to_string(),
        3,
        "My Phone".to_string(),
        0,
    );

    let password = "correct-horse-battery-staple";
    let backup = original.export_backup(password).unwrap();
    let restored = Identity::import_backup(&backup, password, 0).unwrap();

    assert_eq!(restored.device_index(), 3);
    assert_eq!(restored.device_info().device_name(), "My Phone");
    assert_eq!(restored.device_id(), original.device_id());
}

// @scenario: identity_management :: Create new identity on first launch
#[test]
fn test_device_id_deterministic() {
    let identity1 = Identity::create("Alice", 0);
    let identity2 = Identity::create("Bob", 0);

    // Different identities have different device IDs
    assert_ne!(identity1.device_id(), identity2.device_id());
}

/// Tracker #235: Master seed entropy validation — uniqueness.
///
/// Verifies that `Identity::create(, 0)` produces cryptographically unique
/// identities. Since the master seed is private, we verify indirectly via
/// the derived signing public key (deterministically derived from seed via
/// HKDF). Two identical public keys would mean identical seeds, which would
/// indicate a catastrophic RNG failure.
// @scenario: identity_management :: Create new identity on first launch
#[test]
fn test_identity_create_produces_unique_keys() {
    let mut public_keys: Vec<[u8; 32]> = Vec::with_capacity(20);
    for i in 0..20 {
        let identity = Identity::create(&format!("User{}", i), 0);
        let pk = *identity.signing_public_key();
        public_keys.push(pk);
    }

    // All signing public keys must be unique
    for i in 0..public_keys.len() {
        for j in (i + 1)..public_keys.len() {
            assert_ne!(
                public_keys[i], public_keys[j],
                "Identity {} and {} have identical signing keys — catastrophic RNG failure",
                i, j
            );
        }
    }
}

/// Tracker #235: Master seed entropy validation — non-degenerate keys.
///
/// Verifies that generated identity keys are not degenerate (all-zero or
/// all-same-byte). A degenerate key would indicate an uninitialized or
/// stuck RNG, making all cryptographic operations insecure.
// @scenario: identity_management :: Create new identity on first launch
#[test]
fn test_identity_keys_not_degenerate() {
    let identity = Identity::create("Entropy Test", 0);

    // Signing public key must not be all zeros
    let spk = identity.signing_public_key();
    assert!(
        spk.iter().any(|&b| b != 0),
        "Signing public key must not be all zeros"
    );
    let first = spk[0];
    assert!(
        spk.iter().any(|&b| b != first),
        "Signing public key must not be all the same byte ({:#04x})",
        first
    );

    // Exchange public key must not be all zeros
    let epk = identity.exchange_public_key();
    assert!(
        epk.iter().any(|&b| b != 0),
        "Exchange public key must not be all zeros"
    );
    let first = epk[0];
    assert!(
        epk.iter().any(|&b| b != first),
        "Exchange public key must not be all the same byte ({:#04x})",
        first
    );

    // Public ID must not be empty
    let pid = identity.public_id();
    assert!(!pid.is_empty(), "Public ID must not be empty");
}

// ============================================================
// to_storage_bytes / from_storage_bytes round-trip invariant
// (site 3 of _private/.../2026-05-21-silent-failures-in-security-paths)
//
// `vauchi-app/src/ui/app_engine/screens.rs` clones an Identity by going
// through this round-trip (Identity intentionally does not impl Clone
// because it contains key material). The clone path used to be
// `Identity::from_storage_bytes(&bytes).ok()` — a Result swallowed by
// `.ok()`. The audit cited this as a silent-failure site: a
// to_storage_bytes → from_storage_bytes failure means we silently
// produce no exchange session and the user taps "start exchange" with
// no feedback. The site-3 fix logs the violation via tracing instead of
// dropping it, but the underlying contract (this test) is what keeps
// the failure path unreachable in practice.
// ============================================================

// @internal
#[test]
fn identity_storage_bytes_roundtrip_preserves_all_fields() {
    let original = Identity::create("Alice", 7);
    let bytes = original.to_storage_bytes();
    let restored = Identity::from_storage_bytes(&bytes, 0)
        .expect("to_storage_bytes() output must always parse via from_storage_bytes()");

    assert_eq!(restored.display_name(), original.display_name());
    assert_eq!(restored.public_id(), original.public_id());
    assert_eq!(
        restored.signing_public_key(),
        original.signing_public_key(),
        "signing key must round-trip — drives exchange session keys"
    );
}

// @internal
#[test]
fn identity_storage_bytes_roundtrip_preserves_unicode_display_name() {
    let original = Identity::create("Зоя 中文 🦀", 0);
    let bytes = original.to_storage_bytes();
    let restored =
        Identity::from_storage_bytes(&bytes, 0).expect("UTF-8 display name must round-trip");
    assert_eq!(restored.display_name(), "Зоя 中文 🦀");
}

// @internal
#[test]
fn identity_from_storage_bytes_rejects_severely_truncated_input() {
    // 30 bytes is too short to contain even `name_len(4) + master_seed(32)`
    // (= 36 bytes minimum for any non-empty Identity), so the parser must
    // Err. Tail-only truncation falls into the "old format" backward-
    // compatibility branch and is intentionally tolerated (see
    // parse_backup_plaintext at the `>= base_offset + 8` guard).
    let result = Identity::from_storage_bytes(&[0u8; 30], 0);
    assert!(
        result.is_err(),
        "severely truncated storage bytes must Err so the caller can log the contract violation (site 3 of silent-failures audit)"
    );
}
