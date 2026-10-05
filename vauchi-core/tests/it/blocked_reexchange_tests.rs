// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A repeat exchange with a blocked contact is rejected and leaves the block,
//! the stored card and the channel untouched (ADR-056, #295). Both exchange
//! persist seams are covered: with a session ratchet and without one.

use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey, Vauchi, VauchiError};

fn alice_with_blocked(eve: &Identity) -> Vauchi {
    let mut alice = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();
    alice
        .add_contact(Contact::from_exchange(
            *eve.signing_public_key(),
            ContactCard::new("Eve"),
            SymmetricKey::generate(),
            0,
        ))
        .unwrap();
    let id = alice.list_contacts().unwrap()[0].id().to_string();
    alice.block_contact(&id).unwrap();
    alice
}

fn fresh_exchange_with(eve: &Identity) -> (Contact, SymmetricKey) {
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *eve.signing_public_key(),
        ContactCard::new("Eve Again"),
        shared.clone(),
        1,
    );
    (contact, shared)
}

fn assert_untouched(alice: &Vauchi, id: &str) {
    let stored = alice.get_contact(id).unwrap().unwrap();
    assert!(stored.is_blocked(), "the block must survive a re-exchange");
    assert_eq!(
        stored.display_name(),
        "Eve",
        "the stored card must not be replaced"
    );
}

// @scenario: contacts_management :: Blocked contact cannot re-exchange
// @scenario: contact_exchange :: Blocked user attempts exchange
#[test]
fn reexchange_with_a_blocked_contact_is_rejected() {
    let eve = Identity::create("Eve", 0);
    let alice = alice_with_blocked(&eve);
    let (contact, shared) = fresh_exchange_with(&eve);
    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared, *X3DHKeyPair::generate().public_key())
            .unwrap();

    let result = alice.save_exchanged_contact(&contact, &ratchet, true);

    assert!(
        matches!(result, Err(VauchiError::ContactBlocked(ref id)) if id == contact.id()),
        "got {result:?}"
    );
    assert_untouched(&alice, contact.id());
}

// @scenario: contacts_management :: Blocked contact cannot re-exchange
#[test]
fn reexchange_without_a_session_ratchet_is_rejected_too() {
    let eve = Identity::create("Eve", 0);
    let alice = alice_with_blocked(&eve);
    let (contact, _) = fresh_exchange_with(&eve);

    let result = alice.save_exchanged_card(&contact);

    assert!(
        matches!(result, Err(VauchiError::ContactBlocked(ref id)) if id == contact.id()),
        "got {result:?}"
    );
    assert_untouched(&alice, contact.id());
}

// @scenario: contacts_management :: Blocked contact cannot re-exchange
#[test]
fn first_exchange_without_a_session_ratchet_is_saved() {
    let mut alice = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();
    let (contact, _) = fresh_exchange_with(&Identity::create("Eve", 0));

    alice.save_exchanged_card(&contact).unwrap();

    let stored = alice.get_contact(contact.id()).unwrap().unwrap();
    assert_eq!(stored.display_name(), "Eve Again");
    assert!(!stored.is_blocked());
}
