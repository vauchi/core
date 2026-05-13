// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Vauchi::add_personal_note() and read_personal_note()
//!
//! Verifies that note encryption/decryption is handled entirely
//! within core (ADR-021: no crypto in frontends).

use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

/// Helper: create Vauchi with identity and an exchanged contact.
fn setup_with_contact() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let mut pk = [0u8; 32];
    pk[0] = 1;
    let card = ContactCard::new("Bob");
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate(), 0);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    (wb, contact_id)
}

// @scenario: navigation.feature - Add and read personal note roundtrip
#[test]
fn test_add_and_read_note_roundtrip() {
    let (wb, contact_id) = setup_with_contact();

    wb.add_personal_note(&contact_id, "Met at conference 2026")
        .unwrap();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert_eq!(
        note.as_deref(),
        Some("Met at conference 2026"),
        "Decrypted note must match original plaintext"
    );
}

// @scenario: navigation.feature - Read note for contact without note
#[test]
fn test_read_note_returns_none_when_empty() {
    let (wb, contact_id) = setup_with_contact();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert!(note.is_none(), "No note should return None");
}

// @scenario: navigation.feature - Add note overwrites previous
#[test]
fn test_add_note_overwrites_previous() {
    let (wb, contact_id) = setup_with_contact();

    wb.add_personal_note(&contact_id, "First note").unwrap();
    wb.add_personal_note(&contact_id, "Second note").unwrap();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert_eq!(
        note.as_deref(),
        Some("Second note"),
        "Second add must overwrite first"
    );
}

// @scenario: navigation.feature - Note for nonexistent contact fails
#[test]
fn test_add_note_for_missing_contact_fails() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let result = wb.add_personal_note("nonexistent-id", "hello");
    assert!(
        result.is_err(),
        "Adding note to nonexistent contact must fail"
    );
}

// @scenario: navigation.feature - Delete note then read returns None
#[test]
fn test_delete_note_clears() {
    let (wb, contact_id) = setup_with_contact();

    wb.add_personal_note(&contact_id, "To be deleted").unwrap();
    wb.delete_personal_notes(&contact_id).unwrap();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert!(note.is_none(), "Deleted note must return None");
}
