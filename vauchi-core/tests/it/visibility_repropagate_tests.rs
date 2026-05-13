// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for visibility re-propagation.
//!
//! Tests that changing visibility rules via set_field_*_and_repropagate()
//! triggers a new card update to the affected contact.

use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi, VauchiError,
    exchange::X3DHKeyPair,
};

fn create_test_vauchi() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

fn add_contact_with_ratchet(wb: &Vauchi, name: &str) -> String {
    let identity = Identity::create(name, 0);
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

// @scenario: visibility_control :: Revoking visibility sends update to contact
#[test]
fn test_set_field_private_queues_update() {
    let wb = create_test_vauchi();

    // Add a field to own card
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
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

// @scenario: visibility_control :: Granting visibility sends update to contact
#[test]
fn test_set_field_public_queues_update() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+1234567890",
        0,
    ))
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

// @scenario: visibility_control :: Revoking visibility sends update to contact
#[test]
fn test_repropagate_skips_no_ratchet() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    // Add contact WITHOUT ratchet
    let identity = Identity::create("Carol", 0);
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

// @scenario: visibility_control :: Revoking visibility sends update to contact
#[test]
fn test_visibility_change_nonexistent_contact() {
    let wb = create_test_vauchi();

    let result = wb.set_field_private_and_repropagate("nonexistent-id", "work");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "Should return ContactNotFound for nonexistent contact"
    );
}

// @scenario: visibility_control :: Show a field only to specific contacts
#[test]
fn test_set_field_restricted_queues_update() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "personal",
        "alice@personal.com",
        0,
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

// ============================================================
// Label-aware re-propagation tests
// ============================================================

// @scenario: visibility_control :: Add contact to group updates their visibility
#[test]
fn test_add_contact_to_label_triggers_repropagate() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let label = wb.create_group("Work").unwrap();

    // Set field visible in this label
    wb.set_group_field_visibility(label.id(), "work", true)
        .unwrap();

    let pending_before = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(pending_before.is_empty());

    // Add contact to label AND repropagate
    wb.add_contact_to_group_and_repropagate(label.id(), &bob_id)
        .unwrap();

    let pending_after = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending_after.is_empty(),
        "Adding contact to label should queue a re-propagation update"
    );
}

// @scenario: visibility_control :: Remove contact from group updates their visibility
#[test]
fn test_remove_contact_from_label_triggers_repropagate() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let label = wb.create_group("Work").unwrap();

    // Add contact to label first (without repropagate to keep pending clean)
    wb.add_contact_to_group(label.id(), &bob_id).unwrap();

    // Now remove and repropagate
    wb.remove_contact_from_group_and_repropagate(label.id(), &bob_id)
        .unwrap();

    let pending = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending.is_empty(),
        "Removing contact from label should queue a re-propagation update"
    );
}

// @scenario: visibility_control :: Apply visibility group to a field
#[test]
fn test_set_label_field_visibility_repropagates_to_all_members() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let carol_id = add_contact_with_ratchet(&wb, "Carol");

    let label = wb.create_group("Team").unwrap();
    wb.add_contact_to_group(label.id(), &bob_id).unwrap();
    wb.add_contact_to_group(label.id(), &carol_id).unwrap();

    // Set field visibility for the label and repropagate to all members
    wb.set_group_field_visibility_and_repropagate(label.id(), "work", true)
        .unwrap();

    let bob_pending = wb.storage().get_pending_updates(&bob_id).unwrap();
    let carol_pending = wb.storage().get_pending_updates(&carol_id).unwrap();

    assert!(
        !bob_pending.is_empty(),
        "Bob should receive re-propagation update"
    );
    assert!(
        !carol_pending.is_empty(),
        "Carol should receive re-propagation update"
    );
}

// @scenario: visibility_control :: Hide a field from a specific contact
#[test]
fn test_set_contact_override_triggers_repropagate() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "personal",
        "alice@personal.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    let pending_before = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(pending_before.is_empty());

    // Set per-contact override and repropagate
    wb.set_contact_visibility_override_and_repropagate(&bob_id, "personal", false)
        .unwrap();

    let pending_after = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending_after.is_empty(),
        "Per-contact override should queue a re-propagation update"
    );
}

// @scenario: visibility_control :: Granting visibility sends update to contact
// @scenario: visibility_control :: Hide a field from a specific contact
#[test]
fn test_repropagate_uses_effective_visibility() {
    let wb = create_test_vauchi();

    // Add two fields
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();
    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "personal-phone",
        "+1234567890",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    // Create a label that shows only work email
    let label = wb.create_group("Work").unwrap();
    wb.set_group_field_visibility(label.id(), "work", true)
        .unwrap();

    // Add bob to the label and set personal-phone to hidden via override
    wb.add_contact_to_group(label.id(), &bob_id).unwrap();
    wb.set_contact_visibility_override(&bob_id, "personal-phone", false)
        .unwrap();

    // Verify effective visibility
    assert!(
        wb.get_effective_field_visibility(&bob_id, "work").unwrap(),
        "Work field should be visible via label"
    );
    assert!(
        !wb.get_effective_field_visibility(&bob_id, "personal-phone")
            .unwrap(),
        "Personal phone should be hidden via override"
    );

    // Re-propagate using the new label-aware method
    wb.set_field_public_and_repropagate(&bob_id, "work")
        .unwrap();

    // Should have queued an update (the re-propagation uses effective visibility)
    let pending = wb.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending.is_empty(),
        "Re-propagation should queue update using effective visibility"
    );
}
