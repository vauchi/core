// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for visibility re-propagation.
//!
//! Tests that changing visibility rules via set_field_*_and_repropagate()
//! triggers a new card update to the affected contact.

use vauchi_core::network::MockTransport;
use vauchi_core::{
    exchange::X3DHKeyPair, Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey,
    Vauchi, VauchiError,
};

fn create_test_vauchi() -> Vauchi<MockTransport> {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

fn add_contact_with_ratchet(wb: &Vauchi<MockTransport>, name: &str) -> String {
    let identity = Identity::create(name);
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        shared.clone(),
    );
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Set up ratchet as initiator so repropagate can encrypt
    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared, *their_dh.public_key())
        .unwrap();

    contact_id
}

#[test]
fn test_set_field_private_queues_update() {
    let wb = create_test_vauchi();

    // Add a field to own card
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ))
    .unwrap();

    // Add a contact with ratchet
    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    // No pending updates initially
    let pending_before = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        pending_before.is_empty(),
        "No pending updates before visibility change"
    );

    // Set field private and repropagate
    wb.set_field_private_and_repropagate(&bob_id, "work")
        .unwrap();

    // Should have queued an update
    let pending_after = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending_after.is_empty(),
        "Should queue re-propagation update after visibility change"
    );
}

#[test]
fn test_set_field_public_queues_update() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(FieldType::Phone, "mobile", "+1234567890"))
        .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    // Set field public (it's public by default, but this should still trigger repropagate)
    wb.set_field_public_and_repropagate(&bob_id, "mobile")
        .unwrap();

    let pending = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending.is_empty(),
        "Setting field public should queue a re-propagation update"
    );
}

#[test]
fn test_repropagate_skips_no_ratchet() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ))
    .unwrap();

    // Add contact WITHOUT ratchet
    let identity = Identity::create("Carol");
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
    );
    let carol_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Should succeed without error (silently skips)
    let result = wb.set_field_private_and_repropagate(&carol_id, "work");
    assert!(
        result.is_ok(),
        "Re-propagation should silently skip contacts without ratchet"
    );

    // No pending updates (no ratchet to encrypt with)
    let pending = wb.storage().get_pending_updates(&carol_id).unwrap();
    assert!(
        pending.is_empty(),
        "No update should be queued for contact without ratchet"
    );
}

#[test]
fn test_visibility_change_nonexistent_contact() {
    let wb = create_test_vauchi();

    let result = wb.set_field_private_and_repropagate("nonexistent-id", "work");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "Should return ContactNotFound for nonexistent contact"
    );
}

#[test]
fn test_set_field_restricted_queues_update() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "personal",
        "alice@personal.com",
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    wb.set_field_restricted_and_repropagate(
        &bob_id,
        "personal",
        vec!["allowed-contact-1".to_string()],
    )
    .unwrap();

    let pending = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending.is_empty(),
        "Restricted visibility change should queue an update"
    );
}
