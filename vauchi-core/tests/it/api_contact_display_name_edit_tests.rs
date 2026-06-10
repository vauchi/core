// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for editing a contact's displayed name.
//!
//! Editing a contact's display name persists a local, encrypted nickname
//! (the contact's signed card is immutable, and for CEK-protected contacts
//! the plaintext `display_name` column is NULL by ADR-015). The edited name
//! must then surface as the resolved display name on every read path the UI
//! consumes — `get_contact` (detail) and `list_contacts` (list).
//!
//! @scenario: contacts_management.feature - Edit contact display name

use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

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

// @scenario: contacts_management.feature :: Edit contact display name
#[test]
fn editing_display_name_resolves_in_get_contact() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_display_name(&cid, "Bobby").unwrap();

    let contact = wb.get_contact(&cid).unwrap().expect("contact present");
    assert_eq!(
        contact.display_name(),
        "Bobby",
        "detail (get_contact) must show the edited name"
    );
}

// @scenario: contacts_management.feature :: Edit contact display name
#[test]
fn editing_display_name_resolves_in_list_contacts() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_display_name(&cid, "Bobby").unwrap();

    let listed = wb.list_contacts().unwrap();
    let found = listed
        .iter()
        .find(|c| c.id() == cid)
        .expect("contact in list");
    assert_eq!(
        found.display_name(),
        "Bobby",
        "contacts list must show the edited name"
    );
}

// @internal — the signed card is never mutated by a local name edit.
#[test]
fn editing_display_name_leaves_the_signed_card_untouched() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_display_name(&cid, "Bobby").unwrap();

    let contact = wb.get_contact(&cid).unwrap().expect("contact present");
    assert_eq!(
        contact.card().display_name(),
        "Bob",
        "card display name (signed) must remain the primary name"
    );
}

// @internal — the override is the encrypted nickname with a Custom preference.
#[test]
fn editing_display_name_persists_as_nickname() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_display_name(&cid, "Bobby").unwrap();

    assert_eq!(
        wb.get_contact_nickname(&cid).unwrap().as_deref(),
        Some("Bobby"),
        "edit must persist via the encrypted nickname store"
    );
}

// @internal — reverting to the card's primary name clears the override.
#[test]
fn reverting_to_primary_name_clears_the_override() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_display_name(&cid, "Bobby").unwrap();
    wb.set_contact_display_name(&cid, "Bob").unwrap();

    let contact = wb.get_contact(&cid).unwrap().expect("contact present");
    assert_eq!(contact.display_name(), "Bob", "name reverts to primary");
    assert!(
        wb.get_contact_nickname(&cid).unwrap().is_none(),
        "reverting to primary must clear the nickname override"
    );
}

// @internal
#[test]
fn editing_display_name_overwrites_a_previous_edit() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_display_name(&cid, "Bobby").unwrap();
    wb.set_contact_display_name(&cid, "Rob").unwrap();

    let contact = wb.get_contact(&cid).unwrap().expect("contact present");
    assert_eq!(contact.display_name(), "Rob");
}

// @internal
#[test]
fn editing_display_name_rejects_empty_after_trim() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_contact_display_name(&cid, "   ");
    assert!(result.is_err(), "whitespace-only name must be rejected");
}
