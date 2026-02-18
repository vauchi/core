// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for identity
//! Extracted from mod.rs

use vauchi_core::*;

#[test]
fn test_create_identity() {
    let identity = Identity::create("Test User");
    assert_eq!(identity.display_name(), "Test User");
}

#[test]
fn test_backup_restore_roundtrip() {
    let original = Identity::create("Alice");
    let password = "correct-horse-battery-staple";
    let backup = original.export_backup(password).unwrap();
    let restored = Identity::import_backup(&backup, password).unwrap();
    assert_eq!(original.public_id(), restored.public_id());
}

#[test]
fn test_identity_has_device_info() {
    let identity = Identity::create("Alice");
    assert_eq!(identity.device_index(), 0);
    assert_eq!(identity.device_info().device_name(), "Primary Device");
}

#[test]
fn test_backup_restore_preserves_device_info() {
    // Create identity with custom device info using public from_device_link
    let master_seed = [0x42u8; 32];
    let original =
        Identity::from_device_link(master_seed, "Alice".to_string(), 3, "My Phone".to_string());

    let password = "correct-horse-battery-staple";
    let backup = original.export_backup(password).unwrap();
    let restored = Identity::import_backup(&backup, password).unwrap();

    assert_eq!(restored.device_index(), 3);
    assert_eq!(restored.device_info().device_name(), "My Phone");
    assert_eq!(restored.device_id(), original.device_id());
}

#[test]
fn test_device_id_deterministic() {
    let identity1 = Identity::create("Alice");
    let identity2 = Identity::create("Bob");

    // Different identities have different device IDs
    assert_ne!(identity1.device_id(), identity2.device_id());
}

/// Tracker #235: Master seed entropy validation — uniqueness.
///
/// Verifies that `Identity::create()` produces cryptographically unique
/// identities. Since the master seed is private, we verify indirectly via
/// the derived signing public key (deterministically derived from seed via
/// HKDF). Two identical public keys would mean identical seeds, which would
/// indicate a catastrophic RNG failure.
#[test]
fn test_identity_create_produces_unique_keys() {
    let mut public_keys: Vec<[u8; 32]> = Vec::with_capacity(20);
    for i in 0..20 {
        let identity = Identity::create(&format!("User{}", i));
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
#[test]
fn test_identity_keys_not_degenerate() {
    let identity = Identity::create("Entropy Test");

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
