// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact field notes storage CRUD.
//!
//! Covers the `contact_field_notes` table introduced in migration V32.
//! Per-field private annotations: encrypted at rest, never shared.

use vauchi_core::Storage;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn create_test_contact(key_byte: u8, name: &str) -> Contact {
    let public_key = [key_byte; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

// @scenario: contact_field_notes :: save and load round-trip
#[test]
fn test_save_and_load_field_note() {
    let storage = create_test_storage();
    let contact = create_test_contact(0x01, "Alice");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    let note_bytes = b"encrypted-note-data";
    storage
        .save_contact_field_note(&contact_id, "f1", note_bytes)
        .unwrap();

    let notes = storage.load_contact_field_notes(&contact_id).unwrap();

    assert_eq!(notes.len(), 1, "Expected exactly one field note");
    assert_eq!(
        notes.get("f1").expect("f1 note should be present"),
        note_bytes,
        "Loaded note bytes should match saved bytes"
    );
}

// @scenario: contact_field_notes :: empty result for contact with no notes
#[test]
fn test_load_field_notes_empty() {
    let storage = create_test_storage();
    let contact = create_test_contact(0x02, "Bob");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    let notes = storage.load_contact_field_notes(&contact_id).unwrap();

    assert!(
        notes.is_empty(),
        "Expected empty HashMap for contact with no field notes"
    );
}

// @scenario: contact_field_notes :: delete removes specific note
#[test]
fn test_delete_field_note() {
    let storage = create_test_storage();
    let contact = create_test_contact(0x03, "Carol");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    storage
        .save_contact_field_note(&contact_id, "f1", b"some-encrypted-data")
        .unwrap();

    // Verify it's there
    let notes_before = storage.load_contact_field_notes(&contact_id).unwrap();
    assert_eq!(notes_before.len(), 1, "Expected 1 note before delete");

    // Delete it
    storage
        .delete_contact_field_note(&contact_id, "f1")
        .unwrap();

    // Should be gone
    let notes_after = storage.load_contact_field_notes(&contact_id).unwrap();
    assert!(notes_after.is_empty(), "Expected empty after delete");
}

// @scenario: contact_field_notes :: CASCADE on contact delete cleans notes
#[test]
fn test_cascade_on_contact_delete() {
    let storage = create_test_storage();
    let contact = create_test_contact(0x04, "Dave");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();
    storage
        .save_contact_field_note(&contact_id, "f1", b"note-bytes")
        .unwrap();

    // Notes exist
    let notes = storage.load_contact_field_notes(&contact_id).unwrap();
    assert_eq!(notes.len(), 1, "Expected 1 note before contact delete");

    // Delete the contact (CASCADE should remove notes)
    storage.delete_contact(&contact_id).unwrap();

    // Notes should be empty — ON DELETE CASCADE
    let notes_after = storage.load_contact_field_notes(&contact_id).unwrap();
    assert!(
        notes_after.is_empty(),
        "Field notes should be deleted via CASCADE when contact is deleted"
    );
}

// @scenario: contact_field_notes :: INSERT OR REPLACE updates existing note
#[test]
fn test_update_existing_note() {
    let storage = create_test_storage();
    let contact = create_test_contact(0x05, "Eve");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    // Save initial note
    storage
        .save_contact_field_note(&contact_id, "f1", b"original-data")
        .unwrap();

    // Overwrite with updated note
    storage
        .save_contact_field_note(&contact_id, "f1", b"updated-data")
        .unwrap();

    let notes = storage.load_contact_field_notes(&contact_id).unwrap();
    assert_eq!(notes.len(), 1, "Should still be 1 note after update");
    assert_eq!(
        notes.get("f1").expect("f1 should exist"),
        b"updated-data",
        "Note should reflect the latest value"
    );
}

// @scenario: contact_field_notes :: multiple fields on same contact
#[test]
fn test_multiple_field_notes_per_contact() {
    let storage = create_test_storage();
    let contact = create_test_contact(0x06, "Frank");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    storage
        .save_contact_field_note(&contact_id, "field-1", b"note-for-field-1")
        .unwrap();
    storage
        .save_contact_field_note(&contact_id, "field-2", b"note-for-field-2")
        .unwrap();

    let notes = storage.load_contact_field_notes(&contact_id).unwrap();

    assert_eq!(notes.len(), 2, "Expected 2 field notes");
    assert_eq!(
        notes.get("field-1").expect("field-1 should be present"),
        b"note-for-field-1",
        "field-1 note should match"
    );
    assert_eq!(
        notes.get("field-2").expect("field-2 should be present"),
        b"note-for-field-2",
        "field-2 note should match"
    );
}

// @scenario: contact_field_notes :: notes for different contacts are isolated
#[test]
fn test_field_notes_isolated_per_contact() {
    let storage = create_test_storage();
    let contact1 = create_test_contact(0x07, "Grace");
    let contact2 = create_test_contact(0x08, "Henry");
    let id1 = contact1.id().to_string();
    let id2 = contact2.id().to_string();

    storage.save_contact(&contact1).unwrap();
    storage.save_contact(&contact2).unwrap();

    storage
        .save_contact_field_note(&id1, "f1", b"grace-note")
        .unwrap();
    storage
        .save_contact_field_note(&id2, "f1", b"henry-note")
        .unwrap();

    let notes1 = storage.load_contact_field_notes(&id1).unwrap();
    let notes2 = storage.load_contact_field_notes(&id2).unwrap();

    assert_eq!(
        notes1.get("f1").expect("grace f1 should exist"),
        b"grace-note",
        "Grace's note should not be affected by Henry's"
    );
    assert_eq!(
        notes2.get("f1").expect("henry f1 should exist"),
        b"henry-note",
        "Henry's note should not be affected by Grace's"
    );
}

// @scenario: contact_field_notes :: delete only removes specified field, not others
#[test]
fn test_delete_leaves_other_fields_intact() {
    let storage = create_test_storage();
    let contact = create_test_contact(0x09, "Iris");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    storage
        .save_contact_field_note(&contact_id, "f1", b"note-1")
        .unwrap();
    storage
        .save_contact_field_note(&contact_id, "f2", b"note-2")
        .unwrap();

    // Delete only f1
    storage
        .delete_contact_field_note(&contact_id, "f1")
        .unwrap();

    let notes = storage.load_contact_field_notes(&contact_id).unwrap();
    assert_eq!(notes.len(), 1, "Only f2 should remain after deleting f1");
    assert!(
        notes.get("f1").is_none(),
        "f1 should be gone after deletion"
    );
    assert_eq!(
        notes.get("f2").expect("f2 should still be present"),
        b"note-2",
        "f2 should be unaffected by f1 deletion"
    );
}
