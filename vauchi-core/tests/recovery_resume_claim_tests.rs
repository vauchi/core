// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::Vauchi;
#[cfg(feature = "testing")]
use vauchi_core::{StorageError, VauchiError};

// @internal — P32: reading resume state cannot create a recovery or expose it after initialization.
#[test]
fn pending_recovery_claim_requires_identity_free_persisted_recovery() {
    let mut phone = Vauchi::in_memory().unwrap();
    assert_eq!(phone.pending_recovery_claim().unwrap(), None);
    let original = phone.begin_recovery(&"01".repeat(32)).unwrap();
    let original = phone.parse_recovery_claim_b64(&original).unwrap();
    let resumed = phone.pending_recovery_claim().unwrap().unwrap();
    let resumed = phone.parse_recovery_claim_b64(&resumed).unwrap();
    assert_eq!(resumed.old_pk(), original.old_pk());
    assert_eq!(resumed.new_pk(), original.new_pk());
    assert!(!phone.has_identity());

    phone.create_identity("Alice").unwrap();
    assert_eq!(phone.pending_recovery_claim().unwrap(), None);
    assert_eq!(phone.identity().unwrap().display_name(), "Alice");
}

// @internal — P32: resume can read persisted state without rewriting it.
#[cfg(feature = "testing")]
#[test]
fn pending_recovery_claim_works_when_storage_refuses_writes() {
    let phone = Vauchi::in_memory().unwrap();
    let original = phone.begin_recovery(&"01".repeat(32)).unwrap();
    let original = phone.parse_recovery_claim_b64(&original).unwrap();
    phone
        .storage()
        .connection()
        .execute_batch("PRAGMA query_only = ON")
        .unwrap();

    let resumed = phone.pending_recovery_claim().unwrap().unwrap();
    let resumed = phone.parse_recovery_claim_b64(&resumed).unwrap();
    assert_eq!(resumed.old_pk(), original.old_pk());
    assert_eq!(resumed.new_pk(), original.new_pk());
    assert!(!phone.has_identity());
}

// @internal — DC-02: unreadable recovery state must be refused, not replaced with a new key.
#[cfg(feature = "testing")]
#[test]
fn pending_recovery_claim_propagates_corrupt_storage_refusal() {
    let phone = Vauchi::in_memory().unwrap();
    phone.begin_recovery(&"01".repeat(32)).unwrap();
    phone
        .storage()
        .connection()
        .execute("UPDATE recovery_mode_key SET seed_encrypted = X'00'", [])
        .unwrap();

    assert!(matches!(
        phone.pending_recovery_claim(),
        Err(VauchiError::Storage(StorageError::Encryption(_)))
    ));
    assert!(!phone.has_identity());
}
