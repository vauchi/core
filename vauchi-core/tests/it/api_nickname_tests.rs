// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact nickname CRUD operations.
//!
//! @scenario: contacts_management.feature - Set and display custom nickname

use vauchi_core::{Contact, ContactCard, ImportSource, SymmetricKey, Vauchi};

fn setup_with_contact() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let mut pk = [0u8; 32];
    pk[0] = 1;
    let card = ContactCard::new("Bob");
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    (wb, contact_id)
}

fn setup_with_imported_contact() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let card = ContactCard::new("Charlie");
    let contact = Contact::from_import(card, ImportSource::VcardFile, None);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    (wb, contact_id)
}

// @scenario: contacts_management.feature :: Set and display custom nickname
#[test]
fn test_set_and_get_nickname_roundtrip() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    let nick = wb.get_contact_nickname(&cid).unwrap();
    assert_eq!(nick.as_deref(), Some("Bobby"));
}

// @internal
#[test]
fn test_get_nickname_returns_none_when_unset() {
    let (wb, cid) = setup_with_contact();
    let nick = wb.get_contact_nickname(&cid).unwrap();
    assert!(nick.is_none(), "Unset nickname must return None");
}

// @internal
#[test]
fn test_clear_nickname() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.clear_contact_nickname(&cid).unwrap();
    let nick = wb.get_contact_nickname(&cid).unwrap();
    assert!(nick.is_none(), "Cleared nickname must return None");
}

// @internal
#[test]
fn test_nickname_rejects_empty_after_trim() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_contact_nickname(&cid, "   ");
    assert!(result.is_err(), "Whitespace-only nickname must be rejected");
}

// @internal
#[test]
fn test_nickname_rejects_too_long() {
    let (wb, cid) = setup_with_contact();
    let long_name = "a".repeat(101);
    let result = wb.set_contact_nickname(&cid, &long_name);
    assert!(result.is_err(), "Nickname >100 chars must be rejected");
}

// @internal
#[test]
fn test_nickname_trims_whitespace() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "  Bobby  ").unwrap();
    let nick = wb.get_contact_nickname(&cid).unwrap();
    assert_eq!(nick.as_deref(), Some("Bobby"));
}

// @internal
#[test]
fn test_nickname_for_missing_contact_fails() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let result = wb.set_contact_nickname("nonexistent", "Nick");
    assert!(result.is_err(), "Nickname on nonexistent contact must fail");
}

// @scenario: contacts_management.feature :: Set and display custom nickname
#[test]
fn test_nickname_works_on_imported_contact() {
    let (wb, cid) = setup_with_imported_contact();
    wb.set_contact_nickname(&cid, "Chuck").unwrap();
    let nick = wb.get_contact_nickname(&cid).unwrap();
    assert_eq!(nick.as_deref(), Some("Chuck"));
}

// @internal
#[test]
fn test_nickname_overwrites_previous() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.set_contact_nickname(&cid, "Rob").unwrap();
    let nick = wb.get_contact_nickname(&cid).unwrap();
    assert_eq!(nick.as_deref(), Some("Rob"));
}
