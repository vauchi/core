// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Vauchi::prepare_card_update_for_contact() API.
//!
//! Verifies that the API encapsulates delta computation, signing,
//! and ratchet encryption — keeping crypto out of frontends (ADR-021).

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{X3DH, X3DHKeyPair};
use vauchi_core::{Contact, ContactField, FieldType, SymmetricKey, Vauchi};

/// Helper: create Vauchi with identity and own card fields.
fn setup_with_card(name: &str) -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "Work",
        "alice@example.com",
        0,
    ))
    .unwrap();
    wb
}

/// Helper: simulate exchange where Alice is the INITIATOR.
///
/// The initiator can send first (has sending chain), unlike the
/// responder who must receive before sending.
fn exchange_as_initiator(wb: &Vauchi) -> String {
    let alice_identity = wb.identity().unwrap();
    let alice_x3dh = alice_identity.x3dh_keypair();

    // Bob's keys (simulated remote peer)
    let bob_identity = X3DHKeyPair::generate();
    let bob_x3dh = X3DHKeyPair::generate();

    // Alice initiates X3DH toward Bob
    let (shared_secret, _) = X3DH::initiate(&alice_x3dh, bob_x3dh.public_key()).unwrap();

    let card = ContactCard::new("Bob");
    let contact =
        Contact::from_exchange(*bob_identity.public_key(), card, shared_secret.clone(), 0);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Create ratchet as initiator (can send first)
    wb.create_ratchet_as_initiator(&contact_id, &shared_secret, *bob_x3dh.public_key())
        .unwrap();

    contact_id
}

// @scenario: sync.feature - Prepare card update encrypts for contact
#[test]
fn test_prepare_card_update_returns_ciphertext() {
    let wb = setup_with_card("Alice");
    let contact_id = exchange_as_initiator(&wb);

    let empty_card = ContactCard::new("Alice");
    let current_card = wb.storage().load_own_card().unwrap().unwrap();

    let result = wb.prepare_card_update_for_contact(&contact_id, &empty_card, &current_card);

    assert!(result.is_ok(), "prepare_card_update must succeed");
    let ciphertext = result.unwrap();
    assert!(!ciphertext.is_empty(), "ciphertext must be non-empty");
}

// @scenario: sync.feature - Prepare card update advances ratchet state
#[test]
fn test_prepare_card_update_advances_ratchet() {
    let wb = setup_with_card("Alice");
    let contact_id = exchange_as_initiator(&wb);

    let empty_card = ContactCard::new("Alice");
    let current_card = wb.storage().load_own_card().unwrap().unwrap();

    // Two calls with same input must produce different ciphertext (ratchet advances)
    let ct1 = wb
        .prepare_card_update_for_contact(&contact_id, &empty_card, &current_card)
        .unwrap();
    let ct2 = wb
        .prepare_card_update_for_contact(&contact_id, &empty_card, &current_card)
        .unwrap();

    assert_ne!(
        ct1, ct2,
        "Ratchet must produce different ciphertext each call"
    );
}

// @scenario: sync.feature - Prepare card update fails without ratchet
#[test]
fn test_prepare_card_update_requires_ratchet() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    // Add contact without exchange (no ratchet)
    let contact = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let empty_card = ContactCard::new("Alice");
    let current_card = ContactCard::new("Alice");
    let result = wb.prepare_card_update_for_contact(&contact_id, &empty_card, &current_card);

    assert!(result.is_err(), "Must fail without ratchet");
}

// @scenario: sync.feature - Prepare card update fails without identity
#[test]
fn test_prepare_card_update_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();

    let empty_card = ContactCard::new("Alice");
    let current_card = ContactCard::new("Alice");
    let result = wb.prepare_card_update_for_contact("nonexistent", &empty_card, &current_card);

    assert!(result.is_err(), "Must fail without identity");
}

// @scenario: sync.feature - Prepare card update with empty delta returns error
#[test]
fn test_prepare_card_update_empty_delta_returns_error() {
    let wb = setup_with_card("Alice");
    let contact_id = exchange_as_initiator(&wb);

    // Same card for old and new → empty delta
    let card = wb.storage().load_own_card().unwrap().unwrap();
    let result = wb.prepare_card_update_for_contact(&contact_id, &card, &card);

    assert!(
        result.is_err(),
        "Empty delta should return error, not send empty update"
    );
}

// @scenario: sync.feature - Prepare card update rejects blocked contacts
#[test]
fn test_prepare_card_update_rejects_blocked_contact() {
    let wb = setup_with_card("Alice");
    let contact_id = exchange_as_initiator(&wb);

    wb.block_contact(&contact_id).unwrap();

    let empty_card = ContactCard::new("Alice");
    let current_card = wb.storage().load_own_card().unwrap().unwrap();
    let result = wb.prepare_card_update_for_contact(&contact_id, &empty_card, &current_card);

    assert!(result.is_err(), "Must reject blocked contacts");
    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("Blocked"),
        "Error must indicate contact is blocked, got: {:?}",
        err
    );
}
