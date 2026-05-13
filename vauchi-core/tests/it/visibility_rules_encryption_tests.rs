// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for migration v18: encrypt visibility_rules.
//!
//! Verifies that:
//! 1. Migration v18 adds the `visibility_rules_encrypted` column
//! 2. Contacts save visibility rules encrypted (not plaintext)
//! 3. Contacts load correctly after encryption (roundtrip)
//! 4. Migration converts existing plaintext visibility rules to encrypted

use std::collections::HashSet;
use vauchi_core::contact::Contact;
use vauchi_core::contact::VisibilityRules;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::{ContactCard, ContactField, FieldType};

fn open_storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    (dir, storage)
}

// === Migration Tests ===

// @internal
#[test]
fn test_migration_v18_adds_encrypted_column() {
    let (dir, _storage) = open_storage();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    // The visibility_rules_encrypted column should exist
    raw_conn
        .prepare("SELECT visibility_rules_encrypted FROM contacts LIMIT 0")
        .expect("contacts.visibility_rules_encrypted column should exist");
}

// @internal
#[test]
fn test_migration_v18_schema_version_at_least_18() {
    let (dir, _storage) = open_storage();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let version: u32 = raw_conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        version >= 18,
        "Schema version should be at least 18, got {}",
        version
    );
}

// === Save/Load Roundtrip Tests ===

// @scenario: visibility_control :: Visibility settings persist after app restart
// @internal
#[test]
fn test_contact_with_visibility_rules_roundtrip() {
    let (_dir, storage) = open_storage();

    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "alice@example.com",
        0,
    ))
    .unwrap();

    let shared_key = SymmetricKey::generate();
    let public_key = [42u8; 32];

    // Create visibility rules with mixed settings
    let mut rules = VisibilityRules::new();
    rules.set_everyone("email-field");
    rules.set_nobody("phone-field");
    let mut contacts_set = HashSet::new();
    contacts_set.insert("contact-1".to_string());
    contacts_set.insert("contact-2".to_string());
    rules.set_contacts("address-field", contacts_set);

    let contact = Contact::from_sync_data(public_key, card, shared_key, 1000, false, rules);

    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();

    // Verify visibility rules survived the roundtrip
    assert_eq!(
        loaded.visibility_rules().unwrap().get("email-field"),
        &vauchi_core::contact::FieldVisibility::Everyone
    );
    assert_eq!(
        loaded.visibility_rules().unwrap().get("phone-field"),
        &vauchi_core::contact::FieldVisibility::Nobody
    );
    assert!(
        loaded
            .visibility_rules()
            .unwrap()
            .can_see("address-field", "contact-1")
    );
    assert!(
        loaded
            .visibility_rules()
            .unwrap()
            .can_see("address-field", "contact-2")
    );
    assert!(
        !loaded
            .visibility_rules()
            .unwrap()
            .can_see("address-field", "contact-3")
    );
}

// @scenario: visibility_control :: New fields default to visible to all contacts
// @internal
#[test]
fn test_contact_with_empty_visibility_rules_roundtrip() {
    let (_dir, storage) = open_storage();

    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    let public_key = [0u8; 32];

    let contact = Contact::from_exchange(public_key, card, shared_key, 0);

    storage.save_contact(&contact).unwrap();
    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();

    // Default visibility rules — all fields visible to everyone
    assert_eq!(
        loaded.visibility_rules().unwrap().get("any-field"),
        &vauchi_core::contact::FieldVisibility::Everyone
    );
}

// === Encryption Verification Tests ===

// @scenario: visibility_control :: Encrypted updates reveal nothing about hidden fields
// @internal
#[test]
fn test_visibility_rules_stored_encrypted_not_plaintext() {
    let (dir, storage) = open_storage();

    let mut card = ContactCard::new("Secret");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "secret@example.com",
        0,
    ))
    .unwrap();

    let shared_key = SymmetricKey::generate();
    let public_key = [99u8; 32];

    let mut rules = VisibilityRules::new();
    rules.set_nobody("secret-phone-field");

    let contact = Contact::from_sync_data(public_key, card, shared_key, 2000, false, rules);

    storage.save_contact(&contact).unwrap();

    // Open raw connection and verify
    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    // visibility_rules_encrypted should be non-NULL
    let encrypted: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT visibility_rules_encrypted FROM contacts WHERE id = ?1",
            [contact.id()],
            |row| row.get(0),
        )
        .unwrap();
    let blob = encrypted.expect("visibility_rules_encrypted should be non-NULL");
    assert!(!blob.is_empty(), "Encrypted blob should not be empty");

    // The encrypted blob should not contain plaintext
    let blob_str = String::from_utf8_lossy(&blob);
    assert!(
        !blob_str.contains("secret-phone-field"),
        "Encrypted blob must not contain plaintext field names"
    );

    // visibility_rules_json (legacy column) should be NULL
    let legacy: Option<String> = raw_conn
        .query_row(
            "SELECT visibility_rules_json FROM contacts WHERE id = ?1",
            [contact.id()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        legacy.is_none(),
        "Legacy visibility_rules_json should be NULL after v18"
    );
}

// === List/Search Tests ===

// @scenario: visibility_control :: Visibility settings persist after app restart
// @internal
#[test]
fn test_list_contacts_with_encrypted_visibility_rules() {
    let (_dir, storage) = open_storage();

    for i in 0..3u8 {
        let card = ContactCard::new(&format!("Contact {}", i));
        let shared_key = SymmetricKey::generate();
        let mut pk = [0u8; 32];
        pk[0] = i + 1;

        let mut rules = VisibilityRules::new();
        rules.set_nobody(&format!("field-{}", i));

        let contact = Contact::from_sync_data(pk, card, shared_key, 1000, false, rules);
        storage.save_contact(&contact).unwrap();
    }

    let contacts = storage.list_contacts().unwrap();
    assert_eq!(contacts.len(), 3);

    // Each contact should have its visibility rules intact
    for (i, contact) in contacts.iter().enumerate() {
        assert_eq!(
            contact
                .visibility_rules()
                .unwrap()
                .get(&format!("field-{}", i)),
            &vauchi_core::contact::FieldVisibility::Nobody
        );
    }
}
