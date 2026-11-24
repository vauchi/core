// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `Vauchi::save_exchanged_card` upserts an exchanged contact's card
//! "without (re)keying the channel". Given the contact a new exchange
//! produced for a person this phone already holds, it must not replace the
//! stored key, nor what the owner recorded about the contact
//! (vauchi/private#1298).

use vauchi_core::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

const BOB: [u8; 32] = [0xBB; 32];

fn alice_holding_bob() -> (Vauchi, String, [u8; 32]) {
    let mut alice = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();
    let old_key = SymmetricKey::generate();
    let mut bob = Contact::from_exchange(BOB, ContactCard::new("Bob"), old_key.clone(), 0);
    bob.set_hidden(true);
    alice.save_exchanged_card(&bob).unwrap();
    (alice, bob.id().to_owned(), *old_key.as_bytes())
}

fn new_exchange_with_bob() -> Contact {
    Contact::from_exchange(
        BOB,
        ContactCard::new("Bob B."),
        SymmetricKey::generate(),
        100,
    )
}

// @internal
#[test]
fn a_new_exchanges_contact_does_not_replace_the_stored_key() {
    let (alice, bob_id, old_key) = alice_holding_bob();

    alice.save_exchanged_card(&new_exchange_with_bob()).unwrap();

    let stored = alice.get_contact(&bob_id).unwrap().unwrap();
    assert_eq!(
        stored.shared_key().map(|key| *key.as_bytes()),
        Some(old_key)
    );
}

// @internal
#[test]
fn a_new_exchanges_contact_keeps_what_the_owner_set() {
    let (alice, bob_id, _) = alice_holding_bob();

    alice.save_exchanged_card(&new_exchange_with_bob()).unwrap();

    let stored = alice.get_contact(&bob_id).unwrap().unwrap();
    assert!(
        stored.is_hidden(),
        "the owner hid Bob; a re-exchange unhid him"
    );
}

// @internal
#[test]
fn a_new_exchanges_contact_brings_the_new_card() {
    let (alice, bob_id, _) = alice_holding_bob();

    alice.save_exchanged_card(&new_exchange_with_bob()).unwrap();

    let stored = alice.get_contact(&bob_id).unwrap().unwrap();
    assert_eq!(stored.card().display_name(), "Bob B.");
}
