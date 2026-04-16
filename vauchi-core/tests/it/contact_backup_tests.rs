// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for encrypted contact backup export/import (Task 10 / DD-1 local MVP).
//!
//! Verifies that both exchanged and imported contacts survive a round-trip
//! through `export_contact_backup` / `import_contact_backup`, that wrong
//! passwords fail, that empty contact lists work, and that imported contact
//! metadata is preserved faithfully.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{BackupError, ImportSource, export_contact_backup, import_contact_backup};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_exchanged(name: &str) -> Contact {
    let mut public_key = [0u8; 32];
    for (i, &b) in name.as_bytes().iter().enumerate() {
        public_key[i % 32] ^= b;
    }
    // Ensure a unique key per name by XOR-ing a fixed byte
    public_key[31] ^= 0xAB;
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

fn make_imported(name: &str, source: ImportSource) -> Contact {
    let card = ContactCard::new(name);
    Contact::from_import(card, source, Some(format!("uid-{}", name)))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Export and import round-trip for a mixed list (exchanged + imported).
/// Both kinds must survive with their IDs and display names intact.
// @internal
#[test]
fn contact_backup_roundtrip() {
    let alice = make_exchanged("Alice");
    let bob = make_imported("Bob", ImportSource::VcardFile);
    let password = "correct-horse-battery-staple";

    let blob = export_contact_backup(&[alice.clone(), bob.clone()], password).unwrap();
    assert!(!blob.is_empty(), "backup blob must not be empty");

    let restored = import_contact_backup(&blob, password).unwrap();
    assert_eq!(restored.len(), 2, "must restore exactly 2 contacts");

    // Order is preserved
    let r_alice = &restored[0];
    let r_bob = &restored[1];

    assert_eq!(r_alice.id(), alice.id());
    assert_eq!(r_alice.display_name(), alice.display_name());
    assert!(r_alice.is_exchanged(), "Alice must remain exchanged");

    assert_eq!(r_bob.id(), bob.id());
    assert_eq!(r_bob.display_name(), bob.display_name());
    assert!(r_bob.is_imported(), "Bob must remain imported");
}

/// Wrong password must return an error (authentication tag mismatch).
// @internal
#[test]
fn contact_backup_wrong_password_fails() {
    let contact = make_exchanged("Charlie");
    let blob = export_contact_backup(&[contact], "right-password").unwrap();

    let result = import_contact_backup(&blob, "wrong-password");
    assert!(
        matches!(result, Err(BackupError::DecryptionFailed)),
        "expected DecryptionFailed, got {:?}",
        result
    );
}

/// An empty contact list exports and imports without error.
// @internal
#[test]
fn contact_backup_empty_list() {
    let blob = export_contact_backup(&[], "password").unwrap();
    let restored = import_contact_backup(&blob, "password").unwrap();
    assert!(
        restored.is_empty(),
        "importing an empty backup must yield 0 contacts"
    );
}

/// Imported contact metadata (source, imported_at, original_uid) is preserved.
// @internal
#[test]
fn contact_backup_preserves_imported_metadata() {
    let contact = make_imported("Dave", ImportSource::Manual);
    let original_imported = contact.kind().imported_data().unwrap();
    let original_uid = original_imported.original_uid.clone();
    let original_imported_at = original_imported.imported_at;

    let blob = export_contact_backup(std::slice::from_ref(&contact), "s3cr3t").unwrap();
    let restored = import_contact_backup(&blob, "s3cr3t").unwrap();

    assert_eq!(restored.len(), 1);
    let r = &restored[0];
    assert_eq!(r.id(), contact.id(), "ID must be preserved");

    let imp = r
        .kind()
        .imported_data()
        .expect("must be an imported contact");
    assert_eq!(
        imp.original_uid, original_uid,
        "original_uid must be preserved"
    );
    assert_eq!(
        imp.imported_at, original_imported_at,
        "imported_at timestamp must be preserved"
    );
    assert!(
        matches!(imp.source, ImportSource::Manual),
        "ImportSource must round-trip correctly"
    );
}

/// Tampered ciphertext must fail with DecryptionFailed.
// @internal
#[test]
fn contact_backup_tampered_ciphertext_fails() {
    let contact = make_exchanged("Eve");
    let mut blob = export_contact_backup(&[contact], "password").unwrap();

    // Flip a byte in the ciphertext region (after version byte + 16-byte salt)
    let tamper_index = 1 + 16 + 4; // well into the ciphertext
    assert!(
        blob.len() > tamper_index,
        "blob must be long enough to tamper"
    );
    blob[tamper_index] ^= 0xFF;

    let result = import_contact_backup(&blob, "password");
    assert!(
        matches!(result, Err(BackupError::DecryptionFailed)),
        "expected DecryptionFailed after tampering, got {:?}",
        result
    );
}

/// Truncated data returns TooShort error.
// @internal
#[test]
fn contact_backup_truncated_data_fails() {
    let result = import_contact_backup(&[0x01, 0x02], "password");
    assert!(
        matches!(result, Err(BackupError::TooShort)),
        "expected TooShort, got {:?}",
        result
    );
}

/// Unknown version byte returns UnsupportedVersion error.
// @internal
#[test]
fn contact_backup_unknown_version_fails() {
    // Build a blob with an invalid version byte but enough salt+data
    let mut fake = vec![0xFFu8]; // unsupported version
    fake.extend_from_slice(&[0u8; 48]); // salt + some ciphertext-like bytes
    let result = import_contact_backup(&fake, "password");
    assert!(
        matches!(result, Err(BackupError::UnsupportedVersion(0xFF))),
        "expected UnsupportedVersion(0xFF), got {:?}",
        result
    );
}
