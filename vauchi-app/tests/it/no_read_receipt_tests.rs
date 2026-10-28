// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a contact's update tells them nothing (ADR-057: no attention
//! evidence unless the user opts in, and there is no opt-in yet).

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{SymmetricKey, Vauchi};

fn person(name: &str) -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity(name).unwrap();
    vauchi
}

/// Exchanged contacts with a live ratchet; returns (Alice's id at Bob,
/// Bob's id at Alice).
fn exchanged(bob: &Vauchi, alice: &Vauchi) -> (String, String) {
    let secret = SymmetricKey::generate();
    let alice_at_bob = Contact::from_exchange(
        *alice.identity().unwrap().signing_public_key(),
        ContactCard::new("Alice"),
        secret.clone(),
        0,
    );
    let bob_at_alice = Contact::from_exchange(
        *bob.identity().unwrap().signing_public_key(),
        ContactCard::new("Bob"),
        secret.clone(),
        0,
    );
    let (alice_id, bob_id) = (alice_at_bob.id().to_string(), bob_at_alice.id().to_string());
    bob.add_contact(alice_at_bob).unwrap();
    alice.add_contact(bob_at_alice).unwrap();
    let bob_dh = X3DHKeyPair::generate();
    let alice_ratchet =
        DoubleRatchetState::initialize_initiator(&secret, *bob_dh.public_key()).unwrap();
    let bob_ratchet = DoubleRatchetState::initialize_responder(&secret, bob_dh);
    bob.storage()
        .ratchets()
        .save_ratchet_state(&alice_id, &bob_ratchet, false)
        .unwrap();
    alice
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_id, &alice_ratchet, true)
        .unwrap();
    (alice_id, bob_id)
}

// @scenario: message_delivery :: Read receipts are off by default
#[test]
fn opening_a_contact_after_their_update_sends_nothing_back() {
    let bob = person("Bob");
    let mut alice = person("Alice");
    let (alice_id, bob_id) = exchanged(&bob, &alice);
    alice
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 123 45 67",
            0,
        ))
        .unwrap();
    let mobile = alice.own_card().unwrap().unwrap().fields()[0]
        .id()
        .to_string();
    alice.set_own_field_public(&mobile).unwrap();
    alice.run_owed_repropagation().unwrap();
    let update = alice
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap()
        .remove(0);
    let received = process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id,
        &update.payload,
    )
    .unwrap();
    assert!(
        matches!(received, ReceiveOutcome::CardDelta),
        "{received:?}"
    );
    let mut engine = AppEngine::new(bob);

    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: alice_id,
    });

    let queued = engine
        .vauchi()
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(queued.len(), 0, "{queued:?}");
}
