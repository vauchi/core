// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hidden Contact API Tests
//!
//! Tests for the Vauchi API methods: list_hidden_contacts, hide_contact, unhide_contact.
//! References: contacts_management.feature (hidden contacts scenarios)

mod common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::*;

/// Helper to create a contact with a given name and unique public key.
fn create_contact_with_name(name: &str, pk_byte: u8) -> Contact {
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange([pk_byte; 32], card, shared_key)
}

// ============================================================
// list_hidden_contacts
// ============================================================

#[test]
fn test_list_hidden_contacts_empty_initially() {
    let wb = create_vauchi_with_identity("Alice");

    let hidden = wb.list_hidden_contacts().unwrap();
    assert!(hidden.is_empty(), "No contacts should be hidden initially");
}

#[test]
fn test_hidden_contact_appears_in_hidden_list() {
    let wb = create_vauchi_with_identity("Alice");

    let contact = create_contact_with_name("Bob", 1);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Hide the contact
    wb.hide_contact(&contact_id).unwrap();

    // Should appear in hidden list
    let hidden = wb.list_hidden_contacts().unwrap();
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].id(), contact_id);
}

// ============================================================
// hide_contact
// ============================================================

#[test]
fn test_hide_contact_removes_from_main_list() {
    let wb = create_vauchi_with_identity("Alice");

    let contact = create_contact_with_name("Bob", 1);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Verify visible initially
    let contacts = wb.list_contacts().unwrap();
    assert!(contacts.iter().any(|c| c.id() == contact_id));

    // Hide
    wb.hide_contact(&contact_id).unwrap();

    // Verify the contact is hidden (hidden field is set)
    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert!(loaded.is_hidden());

    // Verify NO LONGER in list_contacts()
    let visible = wb.list_contacts().unwrap();
    assert!(
        !visible.iter().any(|c| c.id() == contact_id),
        "Hidden contact must not appear in list_contacts()"
    );

    // Verify appears in hidden list
    let hidden = wb.list_hidden_contacts().unwrap();
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].id(), contact_id);
}

#[test]
fn test_hide_nonexistent_contact_fails() {
    let wb = create_vauchi_with_identity("Alice");

    let result = wb.hide_contact("nonexistent-id");
    assert!(result.is_err(), "Hiding a nonexistent contact should fail");
}

#[test]
fn test_hide_already_hidden_contact_is_idempotent() {
    let wb = create_vauchi_with_identity("Alice");

    let contact = create_contact_with_name("Bob", 1);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Hide twice
    wb.hide_contact(&contact_id).unwrap();
    wb.hide_contact(&contact_id).unwrap();

    // Should still be hidden, no error
    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert!(loaded.is_hidden());

    let hidden = wb.list_hidden_contacts().unwrap();
    assert_eq!(hidden.len(), 1);
}

// ============================================================
// unhide_contact
// ============================================================

#[test]
fn test_unhide_contact_returns_to_main_list() {
    let wb = create_vauchi_with_identity("Alice");

    let contact = create_contact_with_name("Bob", 1);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Hide then unhide
    wb.hide_contact(&contact_id).unwrap();
    wb.unhide_contact(&contact_id).unwrap();

    // Should no longer be hidden
    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert!(!loaded.is_hidden());

    // Should not appear in hidden list
    let hidden = wb.list_hidden_contacts().unwrap();
    assert!(hidden.is_empty());

    // Should reappear in main contact list
    let visible = wb.list_contacts().unwrap();
    assert!(
        visible.iter().any(|c| c.id() == contact_id),
        "Unhidden contact must appear in list_contacts()"
    );
}

#[test]
fn test_unhide_already_visible_contact_is_idempotent() {
    let wb = create_vauchi_with_identity("Alice");

    let contact = create_contact_with_name("Bob", 1);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Unhide a contact that was never hidden
    wb.unhide_contact(&contact_id).unwrap();

    // Should still not be hidden, no error
    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert!(!loaded.is_hidden());

    let hidden = wb.list_hidden_contacts().unwrap();
    assert!(hidden.is_empty());
}
