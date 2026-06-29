// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Characterization: two `create_ratchet_as_initiator` ratchets from one
//! shared secret CANNOT decrypt each other.
//!
//! This is the bug in the CLI mutual-QR / USB exchange flow: both sides run
//! `exchange complete` and both call `create_ratchet_as_initiator`
//! (`cli/src/commands/exchange.rs:198,342,477`). Each initiator derives its
//! sending chain from a FRESH local DH against the peer's exchange key, but a
//! responder's receive chain must key off the SAME keypair the initiator
//! sent to — and an initiator never retains the peer's-side keypair as its
//! own `our_dh`. So the two sending chains key off mismatched DH inputs and
//! neither side can build a matching receive chain. The first card update
//! fails to decrypt (`sync.receive_phase rejected=N` in the CLI repro).
//!
//! The fix is to assign one initiator + one responder (as
//! `ExchangeSession::build_exchange_ratchet` and the mobile multistage path
//! already do); the working asymmetric round-trip is pinned by
//! `exchange_real_x3dh_card_update_tests`. This negative test guards against
//! ever wiring a both-initiator channel again.
//!
//! Problem record: 2026-06-28-sync-delivery-sent-not-received (step 3).
//! Feature: features/sync_updates.feature

use crate::common;

use common::helpers::create_vauchi_with_card;
use vauchi_core::{Contact, FieldType, SymmetricKey, api::CardUpdateError};

// @scenario: sync_updates :: A both-initiator exchange cannot decrypt updates
#[test]
fn both_initiator_setup_cannot_decrypt_card_update() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@old.com")]);

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_x3dh_pub = *alice.identity().unwrap().x3dh_keypair().public_key();
    let bob_x3dh_pub = *bob.identity().unwrap().x3dh_keypair().public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    // One shared X3DH secret, as both sides derive in a real exchange.
    let shared = SymmetricKey::generate();

    let bob_at_alice = Contact::from_exchange(bob_pk, bob_card, shared.clone(), 0);
    let bob_id = bob_at_alice.id().to_string();
    alice.add_contact(bob_at_alice).unwrap();
    let alice_at_bob = Contact::from_exchange(alice_pk, alice_card, shared.clone(), 0);
    let alice_id = alice_at_bob.id().to_string();
    bob.add_contact(alice_at_bob).unwrap();

    // The CLI bug: BOTH sides create the ratchet as INITIATOR, each keyed off
    // the peer's exchange key (cli exchange.rs:198/342/477).
    alice
        .create_ratchet_as_initiator(&bob_id, &shared, bob_x3dh_pub)
        .unwrap();
    bob.create_ratchet_as_initiator(&alice_id, &shared, alice_x3dh_pub)
        .unwrap();

    // Alice (an initiator) sends a card update; Bob (also an initiator, never a
    // responder) cannot derive a matching receive chain.
    let old = alice.own_card().unwrap().unwrap();
    let field_id = old.fields().first().unwrap().id().to_string();
    let mut new = old.clone();
    new.update_field_value(&field_id, "alice@new.com", 1)
        .unwrap();
    alice.update_own_card(&new).unwrap();
    let new = alice.own_card().unwrap().unwrap();
    let encrypted = common::card_update::seal_update_default(&alice, &bob_id, &old, &new);

    let result = vauchi_core::api::process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id,
        &encrypted,
    );
    assert!(
        matches!(result, Err(CardUpdateError::DecryptionFailed)),
        "two initiators must fail to decrypt each other (got {result:?})"
    );

    // The edit never crossed — Bob still holds the pre-update value.
    let stored = bob.get_contact(&alice_id).unwrap().unwrap();
    assert!(
        stored
            .card()
            .fields()
            .iter()
            .all(|f| f.value() != "alice@new.com"),
        "the update must not apply under a broken both-initiator channel"
    );
}
