// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact soft-delete and archive functionality.
//!
//! Covers:
//! - Contact struct: soft_delete, undo_soft_delete, archive, unarchive
//! - Storage layer: persist and filter deleted/archived contacts

use vauchi_core::contact::Contact;
use vauchi_core::contact::ImportSource;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::device_sync::SyncItem;
use vauchi_core::{Identity, Storage, Vauchi, VauchiError};

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

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

fn create_exchanged_contact(name: &str) -> Contact {
    let identity = Identity::create(name);
    Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
    )
}

fn create_imported_contact(name: &str) -> Contact {
    let card = ContactCard::new(name);
    Contact::from_import(card, ImportSource::Manual, None)
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

// ============================================================
// Storage layer tests
// ============================================================

#[test]
fn storage_persists_deleted_at() {
    let storage = create_test_storage();
    let mut contact = create_test_contact("Alice", 0x10);
    let ts = 1_700_000_000u64;
    contact.soft_delete(ts);
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(
        loaded.deleted_at(),
        Some(ts),
        "deleted_at should be persisted and loaded"
    );
    assert!(loaded.is_soft_deleted());
}

#[test]
fn storage_persists_archived_flag() {
    let storage = create_test_storage();
    let mut contact = create_test_contact("Bob", 0x11);
    let ts = 1_700_000_000u64;
    contact.archive(ts);
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert!(loaded.is_archived(), "archived flag should be persisted");
    assert_eq!(
        loaded.archived_at(),
        Some(ts),
        "archived_at should be persisted and loaded"
    );
}

#[test]
fn list_contacts_excludes_soft_deleted() {
    let storage = create_test_storage();

    let contact_a = create_test_contact("Active", 0x20);
    storage.save_contact(&contact_a).unwrap();

    let mut contact_d = create_test_contact("Deleted", 0x21);
    contact_d.soft_delete(1_700_000_000);
    storage.save_contact(&contact_d).unwrap();

    let list = storage.list_contacts().unwrap();
    assert_eq!(list.len(), 1, "list_contacts should exclude soft-deleted");
    assert_eq!(list[0].id(), contact_a.id());
}

#[test]
fn list_contacts_excludes_archived() {
    let storage = create_test_storage();

    let contact_a = create_test_contact("Active", 0x30);
    storage.save_contact(&contact_a).unwrap();

    let mut contact_arch = create_test_contact("Archived", 0x31);
    contact_arch.archive(1_700_000_000);
    storage.save_contact(&contact_arch).unwrap();

    let list = storage.list_contacts().unwrap();
    assert_eq!(list.len(), 1, "list_contacts should exclude archived");
    assert_eq!(list[0].id(), contact_a.id());
}

#[test]
fn list_archived_contacts_returns_only_archived() {
    let storage = create_test_storage();

    let contact_a = create_test_contact("Active", 0x40);
    storage.save_contact(&contact_a).unwrap();

    let mut contact_arch = create_test_contact("Archived", 0x41);
    contact_arch.archive(1_700_000_000);
    storage.save_contact(&contact_arch).unwrap();

    let archived = storage.list_archived_contacts().unwrap();
    assert_eq!(archived.len(), 1, "Should return only archived contacts");
    assert_eq!(archived[0].id(), contact_arch.id());
}

#[test]
fn find_stale_soft_deletes_returns_old_deletions() {
    let storage = create_test_storage();

    // Contact deleted at timestamp 100 (old)
    let mut contact_old = create_test_contact("Old", 0x50);
    contact_old.soft_delete(100);
    storage.save_contact(&contact_old).unwrap();

    // Contact deleted at timestamp 500 (recent)
    let mut contact_new = create_test_contact("New", 0x51);
    contact_new.soft_delete(500);
    storage.save_contact(&contact_new).unwrap();

    // Active contact (not deleted)
    let contact_active = create_test_contact("Active", 0x52);
    storage.save_contact(&contact_active).unwrap();

    // Find deletions older than 300
    let stale = storage.find_stale_soft_deletes(300).unwrap();
    assert_eq!(stale.len(), 1, "Should find exactly one stale deletion");
    assert_eq!(stale[0], contact_old.id());
}

// ============================================================
// Task 4: ContactManager API via Vauchi facade
// ============================================================

#[test]
fn soft_delete_imported_contact_sets_deleted_at() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let imported = create_imported_contact("Bob Imported");
    let id = imported.id().to_string();
    wb.add_contact(imported).unwrap();

    wb.soft_delete_imported_contact(&id).unwrap();

    let loaded = wb.get_contact(&id).unwrap().unwrap();
    assert!(loaded.is_soft_deleted(), "Contact should be soft-deleted");
    assert!(loaded.deleted_at().is_some(), "deleted_at should be set");
}

#[test]
fn undo_delete_imported_contact_clears_deleted_at() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let imported = create_imported_contact("Bob Imported");
    let id = imported.id().to_string();
    wb.add_contact(imported).unwrap();

    wb.soft_delete_imported_contact(&id).unwrap();
    wb.undo_delete_imported_contact(&id).unwrap();

    let loaded = wb.get_contact(&id).unwrap().unwrap();
    assert!(
        !loaded.is_soft_deleted(),
        "Contact should not be soft-deleted after undo"
    );
    assert_eq!(loaded.deleted_at(), None, "deleted_at should be None");
}

#[test]
fn hard_delete_imported_contact_removes_from_storage() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let imported = create_imported_contact("Bob Imported");
    let id = imported.id().to_string();
    wb.add_contact(imported).unwrap();

    wb.hard_delete_imported_contact(&id).unwrap();

    let loaded = wb.get_contact(&id).unwrap();
    assert!(loaded.is_none(), "Contact should be removed from storage");
}

#[test]
fn archive_exchanged_contact_sets_archived() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    wb.archive_contact(&bob_id).unwrap();

    let loaded = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(loaded.is_archived(), "Contact should be archived");
    assert!(loaded.archived_at().is_some(), "archived_at should be set");
}

#[test]
fn unarchive_contact_clears_archived() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    wb.archive_contact(&bob_id).unwrap();
    wb.unarchive_contact(&bob_id).unwrap();

    let loaded = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(!loaded.is_archived(), "Contact should not be archived");
    assert_eq!(loaded.archived_at(), None, "archived_at should be None");
}

#[test]
fn vauchi_list_archived_contacts_excludes_from_active() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    let carol = create_exchanged_contact("Carol");
    wb.add_contact(carol).unwrap();

    wb.archive_contact(&bob_id).unwrap();

    let archived = wb.list_archived_contacts().unwrap();
    assert_eq!(
        archived.len(),
        1,
        "Should have exactly one archived contact"
    );
    assert_eq!(archived[0].id(), bob_id);

    // Active list should exclude archived
    let active = wb.list_contacts().unwrap();
    assert_eq!(active.len(), 1, "Active list should exclude archived");
    assert_ne!(active[0].id(), bob_id);
}

// ============================================================
// Task 6: Adversarial tests
// ============================================================

#[test]
fn delete_nonexistent_contact_returns_error() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let result = wb.soft_delete_imported_contact("nonexistent-id");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "Should return ContactNotFound, got: {:?}",
        result
    );
}

#[test]
fn delete_already_deleted_contact_is_idempotent() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let imported = create_imported_contact("Bob");
    let id = imported.id().to_string();
    wb.add_contact(imported).unwrap();

    wb.soft_delete_imported_contact(&id).unwrap();
    let first_ts = wb.get_contact(&id).unwrap().unwrap().deleted_at().unwrap();

    // Second soft-delete should succeed (idempotent) and update timestamp
    wb.soft_delete_imported_contact(&id).unwrap();
    let second_ts = wb.get_contact(&id).unwrap().unwrap().deleted_at().unwrap();

    assert!(
        second_ts >= first_ts,
        "Second delete timestamp should be >= first"
    );
    assert!(
        wb.get_contact(&id).unwrap().unwrap().is_soft_deleted(),
        "Contact should still be soft-deleted"
    );
}

#[test]
fn archive_imported_contact_returns_error() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let imported = create_imported_contact("Bob");
    let id = imported.id().to_string();
    wb.add_contact(imported).unwrap();

    let result = wb.archive_contact(&id);
    assert!(
        matches!(result, Err(VauchiError::InvalidState(_))),
        "Archiving imported contact should return InvalidState, got: {:?}",
        result
    );
}

#[test]
fn delete_exchanged_contact_returns_error() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    let result = wb.soft_delete_imported_contact(&bob_id);
    assert!(
        matches!(result, Err(VauchiError::InvalidState(_))),
        "Soft-deleting exchanged contact should return InvalidState, got: {:?}",
        result
    );
}

#[test]
fn unarchive_non_archived_contact_is_idempotent() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    // Unarchiving a non-archived contact should succeed (idempotent)
    wb.unarchive_contact(&bob_id).unwrap();

    let loaded = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(!loaded.is_archived(), "Contact should not be archived");
    assert_eq!(loaded.archived_at(), None);
}

#[test]
fn undo_delete_after_hard_delete_returns_error() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let imported = create_imported_contact("Bob");
    let id = imported.id().to_string();
    wb.add_contact(imported).unwrap();

    // Soft-delete then hard-delete
    wb.soft_delete_imported_contact(&id).unwrap();
    wb.hard_delete_imported_contact(&id).unwrap();

    // Undo should fail because contact no longer exists
    let result = wb.undo_delete_imported_contact(&id);
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "Undo after hard-delete should return ContactNotFound, got: {:?}",
        result
    );
}

// ============================================================
// Task 5: SyncItem roundtrip serialization tests
// ============================================================

#[test]
fn sync_item_contact_archived_roundtrip() {
    let item = SyncItem::ContactArchived {
        contact_id: "abc123".to_string(),
        timestamp: 1_700_000_000,
    };

    let json = item.to_json();
    let deserialized = SyncItem::from_json(&json).unwrap();

    assert_eq!(item, deserialized, "Roundtrip should preserve equality");
    assert_eq!(
        deserialized.timestamp(),
        1_700_000_000,
        "Timestamp should be preserved"
    );

    // Verify the contact_id is in the JSON
    assert!(
        json.contains("abc123"),
        "JSON should contain the contact_id"
    );
}

#[test]
fn sync_item_contact_unarchived_roundtrip() {
    let item = SyncItem::ContactUnarchived {
        contact_id: "def456".to_string(),
        timestamp: 1_700_000_500,
    };

    let json = item.to_json();
    let deserialized = SyncItem::from_json(&json).unwrap();

    assert_eq!(item, deserialized, "Roundtrip should preserve equality");
    assert_eq!(
        deserialized.timestamp(),
        1_700_000_500,
        "Timestamp should be preserved"
    );

    assert!(
        json.contains("def456"),
        "JSON should contain the contact_id"
    );
}

#[test]
fn apply_sync_contact_archived_sets_flag() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    let items = vec![SyncItem::ContactArchived {
        contact_id: bob_id.clone(),
        timestamp: 1_700_000_000,
    }];

    let applied = wb.apply_sync_items(items).unwrap();
    assert_eq!(applied, 1, "Should apply 1 item");

    let loaded = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(
        loaded.is_archived(),
        "Contact should be archived after sync"
    );
    assert_eq!(
        loaded.archived_at(),
        Some(1_700_000_000),
        "archived_at should match sync timestamp"
    );
}

#[test]
fn apply_sync_contact_unarchived_clears_flag() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    // First archive
    wb.archive_contact(&bob_id).unwrap();

    // Then unarchive via sync
    let items = vec![SyncItem::ContactUnarchived {
        contact_id: bob_id.clone(),
        timestamp: 1_700_001_000,
    }];

    let applied = wb.apply_sync_items(items).unwrap();
    assert_eq!(applied, 1, "Should apply 1 item");

    let loaded = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(
        !loaded.is_archived(),
        "Contact should not be archived after sync unarchive"
    );
    assert_eq!(loaded.archived_at(), None);
}

#[test]
fn apply_sync_archive_nonexistent_contact_skips() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let items = vec![SyncItem::ContactArchived {
        contact_id: "nonexistent".to_string(),
        timestamp: 1_700_000_000,
    }];

    let applied = wb.apply_sync_items(items).unwrap();
    assert_eq!(
        applied, 1,
        "Should still count as applied (skip is non-fatal)"
    );
}
