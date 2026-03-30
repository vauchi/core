// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact soft-delete and archive functionality.
//!
//! Covers:
//! - Contact struct: soft_delete, undo_soft_delete, archive, unarchive
//! - Storage layer: persist and filter deleted/archived contacts

use vauchi_core::Storage;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn create_test_contact(name: &str, key_byte: u8) -> Contact {
    let public_key = [key_byte; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

// ============================================================
// Contact struct: Soft-delete tests
// ============================================================

#[test]
fn new_contact_is_not_soft_deleted() {
    let contact = create_test_contact("Alice", 0x01);
    assert!(
        !contact.is_soft_deleted(),
        "New contact should not be soft-deleted"
    );
    assert_eq!(
        contact.deleted_at(),
        None,
        "New contact deleted_at should be None"
    );
}

#[test]
fn soft_delete_sets_timestamp() {
    let mut contact = create_test_contact("Bob", 0x02);
    let ts = 1_700_000_000;
    contact.soft_delete(ts);
    assert!(
        contact.is_soft_deleted(),
        "Contact should be soft-deleted after soft_delete()"
    );
    assert_eq!(
        contact.deleted_at(),
        Some(ts),
        "deleted_at should match the provided timestamp"
    );
}

#[test]
fn undo_soft_delete_clears_timestamp() {
    let mut contact = create_test_contact("Charlie", 0x03);
    contact.soft_delete(1_700_000_000);
    assert!(contact.is_soft_deleted());

    contact.undo_soft_delete();
    assert!(
        !contact.is_soft_deleted(),
        "Contact should not be soft-deleted after undo"
    );
    assert_eq!(
        contact.deleted_at(),
        None,
        "deleted_at should be None after undo"
    );
}

// ============================================================
// Contact struct: Archive tests
// ============================================================

#[test]
fn new_contact_is_not_archived() {
    let contact = create_test_contact("Dave", 0x04);
    assert!(!contact.is_archived(), "New contact should not be archived");
    assert_eq!(
        contact.archived_at(),
        None,
        "New contact archived_at should be None"
    );
}

#[test]
fn archive_sets_flag_and_timestamp() {
    let mut contact = create_test_contact("Eve", 0x05);
    let ts = 1_700_000_000;
    contact.archive(ts);
    assert!(
        contact.is_archived(),
        "Contact should be archived after archive()"
    );
    assert_eq!(
        contact.archived_at(),
        Some(ts),
        "archived_at should match the provided timestamp"
    );
}

#[test]
fn unarchive_clears_flag_and_timestamp() {
    let mut contact = create_test_contact("Frank", 0x06);
    contact.archive(1_700_000_000);
    assert!(contact.is_archived());

    contact.unarchive();
    assert!(
        !contact.is_archived(),
        "Contact should not be archived after unarchive"
    );
    assert_eq!(
        contact.archived_at(),
        None,
        "archived_at should be None after unarchive"
    );
}

#[test]
fn imported_contact_defaults_not_deleted_not_archived() {
    let card = ContactCard::new("Grace");
    let contact = Contact::from_import(card, vauchi_core::contact::ImportSource::Manual, None);
    assert!(!contact.is_soft_deleted());
    assert!(!contact.is_archived());
    assert_eq!(contact.deleted_at(), None);
    assert_eq!(contact.archived_at(), None);
}
