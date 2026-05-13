// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for full backup wiring through the Vauchi API.
//!
//! These tests verify that `export_full_backup_api()` / `import_full_backup_api()`
//! on the Vauchi struct correctly orchestrate data gathering from storage,
//! encryption, and restoration — the missing wiring layer.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{ContactField, FieldType, ImportSource, Vauchi};

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple";

/// Helper: create a Vauchi instance with identity + contacts + own card.
fn setup_vauchi_with_data() -> Vauchi {
    let mut v = Vauchi::in_memory().unwrap();
    v.create_identity("Alice Smith").unwrap();

    // Add fields to own card
    v.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();
    v.add_own_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15559876543",
        0,
    ))
    .unwrap();

    // Add exchanged contacts via storage
    let card_bob = ContactCard::new("Bob");
    let key_bob = SymmetricKey::generate();
    let bob = Contact::from_exchange([0xBB; 32], card_bob, key_bob, 0);
    v.update_contact(&bob).unwrap();

    let card_carol = ContactCard::new("Carol");
    let key_carol = SymmetricKey::generate();
    let carol = Contact::from_exchange([0xCC; 32], card_carol, key_carol, 0);
    v.update_contact(&carol).unwrap();

    // Add an imported contact
    let card_dave = ContactCard::new("Dave");
    let dave = Contact::from_import(
        card_dave,
        ImportSource::VcardFile,
        Some("uid-dave".into()),
        0,
    );
    v.update_contact(&dave).unwrap();

    v
}

// ── Round-trip through Vauchi API ──────────────────────────────────────────

/// Full backup via Vauchi API: export → import on fresh instance → verify all data.
// @scenario: backup_format_versioning :: Full backup round-trip via API
#[test]
fn full_backup_api_roundtrip() {
    let v = setup_vauchi_with_data();

    // Export full backup
    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();
    assert!(!backup_hex.is_empty());

    // Import on a fresh Vauchi instance
    let mut v2 = Vauchi::in_memory().unwrap();
    v2.import_full_backup(&backup_hex, BACKUP_PASSWORD).unwrap();

    // Identity must be restored
    assert_eq!(v2.public_id().unwrap(), v.public_id().unwrap());
    assert!(v2.identity().is_some());

    // Own card must be restored with fields
    let restored_card = v2.own_card().unwrap().unwrap();
    assert_eq!(restored_card.fields().len(), 2);

    // Contacts must be restored
    let contacts = v2.list_contacts().unwrap();
    assert_eq!(contacts.len(), 3, "expected 3 contacts (Bob, Carol, Dave)");

    let exchanged_count = contacts.iter().filter(|c| c.is_exchanged()).count();
    let imported_count = contacts.iter().filter(|c| c.is_imported()).count();
    assert_eq!(exchanged_count, 2);
    assert_eq!(imported_count, 1);
}

/// Full backup with wrong password must fail.
// @scenario: backup_format_versioning :: Full backup wrong password
#[test]
fn full_backup_api_wrong_password() {
    let v = setup_vauchi_with_data();
    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();

    let mut v2 = Vauchi::in_memory().unwrap();
    let result = v2.import_full_backup(&backup_hex, "wrong-password!");
    assert!(result.is_err());
}

/// Full backup on uninitialized identity must fail.
// @scenario: backup_format_versioning :: Full backup requires identity
#[test]
fn full_backup_api_no_identity() {
    let v = Vauchi::in_memory().unwrap();
    let result = v.export_full_backup(BACKUP_PASSWORD);
    assert!(result.is_err());
}

/// Full backup with zero contacts still works (identity-only content).
// @scenario: backup_format_versioning :: Full backup with empty contacts
#[test]
fn full_backup_api_no_contacts() {
    let mut v = Vauchi::in_memory().unwrap();
    v.create_identity("Solo User").unwrap();

    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();

    let mut v2 = Vauchi::in_memory().unwrap();
    v2.import_full_backup(&backup_hex, BACKUP_PASSWORD).unwrap();

    assert_eq!(v2.public_id().unwrap(), v.public_id().unwrap());
    let contacts = v2.list_contacts().unwrap();
    assert!(contacts.is_empty());
}

/// Import full backup on instance that already has identity must fail.
// @scenario: backup_format_versioning :: Full backup import rejects existing identity
#[test]
fn full_backup_api_import_rejects_existing_identity() {
    let v = setup_vauchi_with_data();
    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();

    // Create a second instance WITH an identity already
    let mut v2 = Vauchi::in_memory().unwrap();
    v2.create_identity("Already Here").unwrap();

    let result = v2.import_full_backup(&backup_hex, BACKUP_PASSWORD);
    assert!(result.is_err(), "import should reject when identity exists");
}
