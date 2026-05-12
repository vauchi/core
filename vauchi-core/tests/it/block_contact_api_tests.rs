// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the block/unblock contact API.
//!
//! Tests that Vauchi.block_contact(), unblock_contact(), and list_blocked_contacts()
//! work correctly, and that blocked contacts are enforced during card propagation
//! and incoming update processing.

use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi, VauchiError,
    crypto::ratchet::DoubleRatchetState, exchange::X3DHKeyPair, sync::delta::CardDelta,
};

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

fn create_contact(name: &str) -> Contact {
    let identity = Identity::create(name);
    Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
    )
}

// === Block / Unblock Basic API ===

// @scenario: contacts_management :: Block a contact
#[test]
fn test_block_contact() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    // Block
    wb.block_contact(&bob_id).unwrap();

    // Verify blocked
    let contact = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(contact.is_blocked(), "Contact should be blocked");
}

// @scenario: contacts_management :: Unblock a contact
#[test]
fn test_unblock_contact() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    // Block then unblock
    wb.block_contact(&bob_id).unwrap();
    wb.unblock_contact(&bob_id).unwrap();

    // Verify unblocked
    let contact = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(!contact.is_blocked(), "Contact should be unblocked");
}

// @scenario: contacts_management :: Block a contact
#[test]
fn test_block_nonexistent_contact() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let result = wb.block_contact("nonexistent-id");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "Blocking nonexistent contact should return ContactNotFound"
    );
}

// @scenario: contacts_management :: Unblock a contact
#[test]
fn test_unblock_nonexistent_contact() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let result = wb.unblock_contact("nonexistent-id");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "Unblocking nonexistent contact should return ContactNotFound"
    );
}

// @scenario: contacts_management :: View blocked contacts
#[test]
fn test_list_blocked_contacts() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    let carol = create_contact("Carol");
    let carol_id = carol.id().to_string();
    wb.add_contact(carol).unwrap();

    let dave = create_contact("Dave");
    let dave_id = dave.id().to_string();
    wb.add_contact(dave).unwrap();

    // No contacts blocked initially
    let blocked = wb.list_blocked_contacts().unwrap();
    assert!(
        blocked.is_empty(),
        "No contacts should be blocked initially"
    );

    // Block Bob and Dave
    wb.block_contact(&bob_id).unwrap();
    wb.block_contact(&dave_id).unwrap();

    let blocked = wb.list_blocked_contacts().unwrap();
    assert_eq!(blocked.len(), 2, "Should have 2 blocked contacts");
    let blocked_ids: Vec<String> = blocked.iter().map(|c| c.id().to_string()).collect();
    assert!(blocked_ids.contains(&bob_id));
    assert!(blocked_ids.contains(&dave_id));
    assert!(!blocked_ids.contains(&carol_id));
}

// @scenario: contacts_management :: Unblock a contact
// @scenario: contacts_management :: View blocked contacts
#[test]
fn test_list_blocked_after_unblock() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let bob = create_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    wb.block_contact(&bob_id).unwrap();
    assert_eq!(wb.list_blocked_contacts().unwrap().len(), 1);

    wb.unblock_contact(&bob_id).unwrap();
    assert_eq!(
        wb.list_blocked_contacts().unwrap().len(),
        0,
        "Unblocked contact should no longer appear in blocked list"
    );
}

// === Blocked Contact Enforcement ===

// @scenario: contacts_management :: Blocked contact cannot re-exchange
#[test]
fn test_blocked_contact_update_rejected() {
    let mut alice = create_test_vauchi();
    alice.create_identity("Alice").unwrap();

    let bob_identity = Identity::create("Bob");
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();

    let bob_contact = Contact::from_exchange(
        *bob_identity.signing_public_key(),
        ContactCard::new("Bob"),
        shared_secret.clone(),
    );
    let bob_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();

    // Set up ratchet so we can process updates
    alice
        .create_ratchet_as_responder(
            &bob_id,
            &shared_secret,
            X3DHKeyPair::from_bytes(*bob_dh.secret_bytes()),
        )
        .unwrap();

    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Create a valid encrypted update from Bob
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob Updated");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "work",
            "bob@work.com",
            0,
        ))
        .unwrap();
    let mut delta = CardDelta::compute(&old_card, &new_card);
    let alice_pk = alice.identity().unwrap().signing_public_key();
    delta.sign(&bob_identity, alice_pk);
    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&delta_bytes).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Block Bob before processing
    alice.block_contact(&bob_id).unwrap();

    // Process should fail with ContactBlocked
    let result = alice.process_card_update(&bob_id, &encrypted);
    assert!(
        matches!(result, Err(VauchiError::ContactBlocked(_))),
        "Updates from blocked contacts should be rejected with ContactBlocked, got: {:?}",
        result.err()
    );
}

// @scenario: contacts_management :: Block a contact
#[test]
fn test_propagate_skips_blocked_contacts() {
    let mut alice = create_test_vauchi();
    alice.create_identity("Alice").unwrap();

    // Add Bob (will be blocked) and Carol (will remain unblocked)
    let bob_identity = Identity::create("Bob");
    let carol_identity = Identity::create("Carol");
    let shared = SymmetricKey::generate();

    let bob = Contact::from_exchange(
        *bob_identity.signing_public_key(),
        ContactCard::new("Bob"),
        shared.clone(),
    );
    let bob_id = bob.id().to_string();
    alice.add_contact(bob).unwrap();

    let carol = Contact::from_exchange(
        *carol_identity.signing_public_key(),
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
    );
    let carol_id = carol.id().to_string();
    alice.add_contact(carol).unwrap();

    // Set up ratchets for both
    let bob_dh = X3DHKeyPair::generate();
    alice
        .create_ratchet_as_initiator(&bob_id, &shared, *bob_dh.public_key())
        .unwrap();

    let carol_dh = X3DHKeyPair::generate();
    let carol_secret = SymmetricKey::generate();
    alice
        .create_ratchet_as_initiator(&carol_id, &carol_secret, *carol_dh.public_key())
        .unwrap();

    // Block Bob
    alice.block_contact(&bob_id).unwrap();

    // Propagate a card update
    let old_card = alice.own_card().unwrap().unwrap();
    alice
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
            0,
        ))
        .unwrap();
    let new_card = alice.own_card().unwrap().unwrap();

    let queued = alice.propagate_card_update(&old_card, &new_card).unwrap();

    // Only Carol should get the update
    assert_eq!(queued, 1, "Only unblocked contacts should receive updates");

    // Verify: Bob has no pending updates, Carol does
    let bob_pending = alice.storage().get_pending_updates(&bob_id).unwrap();
    assert!(
        bob_pending.is_empty(),
        "Blocked Bob should have no pending updates"
    );

    let carol_pending = alice.storage().get_pending_updates(&carol_id).unwrap();
    assert!(
        !carol_pending.is_empty(),
        "Unblocked Carol should have pending updates"
    );
}
