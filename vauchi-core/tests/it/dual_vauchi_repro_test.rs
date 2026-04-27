// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-instance regression coverage for the dual-`Vauchi` pattern used
//! by the mobile bindings (`VauchiPlatform` + `PlatformAppEngine` each
//! own a separate internal `Vauchi` pointing at the same DB).
//!
//! Locks in the storage-fallback `has_identity` self-heal (core!686) and
//! `refresh_identity_from_storage` (core!691) so a future refactor can't
//! silently re-introduce the "AppEngine never sees the identity created
//! by VauchiPlatform" regression class.

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;

// @internal
#[test]
fn dual_vauchi_instance_sees_each_others_identity_writes() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    // Both instances open the same DB simultaneously.
    let config_a = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
    let mut vauchi_a = Vauchi::new(config_a).unwrap();

    let config_b = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
    let mut vauchi_b = Vauchi::new(config_b).unwrap();

    // Both initially see no identity.
    assert!(!vauchi_a.has_identity());
    assert!(!vauchi_b.has_identity());

    // Instance A creates identity.
    vauchi_a.create_identity("Alice").unwrap();
    assert!(vauchi_a.has_identity());

    // Instance B should now also see identity (storage fallback).
    assert!(
        vauchi_b.has_identity(),
        "vauchi_b should see identity via storage fallback after vauchi_a created it"
    );

    // After refresh, vauchi_b also has identity loaded in-memory.
    vauchi_b.refresh_identity_from_storage();
    assert!(
        vauchi_b.has_identity(),
        "vauchi_b should still see identity after refresh"
    );
}

/// Mimics the iOS/Android pattern more precisely: persistent Vauchi B is
/// held for the whole flow (like PlatformAppEngine's internal Vauchi
/// opened once at app launch) while a short-lived Vauchi A is created,
/// used to make identity, and dropped (mirrors the per-operation Storage
/// handle that VauchiPlatform.createIdentity opens).
// @internal
#[test]
fn persistent_vauchi_b_sees_short_lived_vauchi_a_writes() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    let config_b = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
    let mut vauchi_b = Vauchi::new(config_b).unwrap();
    assert!(!vauchi_b.has_identity(), "B should start with no identity");

    {
        let config_a = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
        let mut vauchi_a = Vauchi::new(config_a).unwrap();
        vauchi_a.create_identity("Alice").unwrap();
        assert!(vauchi_a.has_identity());
        // vauchi_a dropped here — its connection closes.
    }

    assert!(
        vauchi_b.has_identity(),
        "B's persistent connection must see A's committed write via the storage fallback"
    );

    vauchi_b.refresh_identity_from_storage();
    let card = vauchi_b
        .own_card()
        .expect("own_card returned err")
        .expect("own_card returned None despite identity in storage");
    assert_eq!(card.display_name(), "Alice");
}
