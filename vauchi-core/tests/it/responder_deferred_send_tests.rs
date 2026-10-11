// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A freshly exchanged responder cannot send until the initiator's first
//! message arrives. A card change made in that window is owed, not failed
//! (vauchi/private#294, #523).

use vauchi_core::api::Vauchi;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType};

use crate::common::helpers::create_vauchi_with_identity;

/// Alice is the exchange initiator and Bob the responder; neither has sent.
struct FreshExchange {
    alice: Vauchi,
    bob: Vauchi,
    bob_at_alice: String,
    alice_at_bob: String,
}

fn fresh_exchange() -> FreshExchange {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let shared = SymmetricKey::generate();

    let bob_contact = Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared.clone(), 0);
    let bob_at_alice = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();
    let alice_contact =
        Contact::from_exchange(alice_pk, ContactCard::new("Alice"), shared.clone(), 0);
    let alice_at_bob = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    let bob_dh = X3DHKeyPair::generate();
    alice
        .create_ratchet_as_initiator(&bob_at_alice, &shared, *bob_dh.public_key())
        .unwrap();
    bob.create_ratchet_as_responder(&alice_at_bob, &shared, bob_dh)
        .unwrap();

    FreshExchange {
        alice,
        bob,
        bob_at_alice,
        alice_at_bob,
    }
}

fn own_field_id(wb: &Vauchi, label: &str) -> String {
    wb.own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|field| field.label() == label)
        .unwrap()
        .id()
        .to_string()
}

fn pending_payloads(wb: &Vauchi, contact_id: &str) -> Vec<Vec<u8>> {
    wb.storage()
        .pending()
        .get_pending_updates(contact_id)
        .unwrap()
        .into_iter()
        .map(|update| update.payload)
        .collect()
}

/// Delivers everything `from` has queued for `contact_at_from` to `to`.
fn deliver(from: &Vauchi, contact_at_from: &str, to: &Vauchi, sender_at_to: &str) {
    for payload in pending_payloads(from, contact_at_from) {
        vauchi_core::api::process_single_card_update(
            to.identity().unwrap(),
            to.storage(),
            sender_at_to,
            &payload,
        )
        .unwrap();
    }
}

/// Bob adds a work email and shows it to Alice before Alice has sent anything.
fn bob_shows_alice_an_email_early(world: &FreshExchange) -> vauchi_core::api::VauchiResult<()> {
    world
        .bob
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "bob@example.com",
            0,
        ))
        .unwrap();
    let email = own_field_id(&world.bob, "work");
    world
        .bob
        .set_contact_visibility_override_and_repropagate(&world.alice_at_bob, &email, true)
}

// @internal
#[test]
fn a_responder_can_show_a_field_before_the_initiator_has_sent() {
    let world = fresh_exchange();

    let result = bob_shows_alice_an_email_early(&world);

    assert!(
        matches!(result, Ok(())),
        "showing a field while the first message is in flight is owed, not an error: {result:?}"
    );
    assert_eq!(pending_payloads(&world.bob, &world.alice_at_bob).len(), 0);
}

// @internal
#[test]
fn waiting_for_the_first_message_does_not_spend_the_retry_budget() {
    let world = fresh_exchange();
    let _ = bob_shows_alice_an_email_early(&world);

    for _ in 0..8 {
        world.bob.run_owed_repropagation().unwrap();
    }

    let owed = world
        .bob
        .storage()
        .ux()
        .load_own_card_repropagate()
        .unwrap();
    assert_eq!(
        owed.failed_attempts, 0,
        "a responder waiting for its first message has not failed"
    );
}

// @internal
#[test]
fn a_field_shown_early_reaches_the_initiator_after_its_first_message() {
    let world = fresh_exchange();
    let _ = bob_shows_alice_an_email_early(&world);
    for _ in 0..8 {
        world.bob.run_owed_repropagation().unwrap();
    }

    world
        .alice
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+41000000000",
            0,
        ))
        .unwrap();
    let phone = own_field_id(&world.alice, "mobile");
    world
        .alice
        .set_contact_visibility_override_and_repropagate(&world.bob_at_alice, &phone, true)
        .unwrap();
    deliver(
        &world.alice,
        &world.bob_at_alice,
        &world.bob,
        &world.alice_at_bob,
    );
    world.bob.run_owed_repropagation().unwrap();
    deliver(
        &world.bob,
        &world.alice_at_bob,
        &world.alice,
        &world.bob_at_alice,
    );

    let bob_at_alice = world
        .alice
        .get_contact(&world.bob_at_alice)
        .unwrap()
        .unwrap();
    let emails: Vec<&str> = bob_at_alice
        .card()
        .fields()
        .iter()
        .filter(|field| field.label() == "work")
        .map(|field| field.value())
        .collect();
    assert_eq!(emails, vec!["bob@example.com"]);
    let owed = world
        .bob
        .storage()
        .ux()
        .load_own_card_repropagate()
        .unwrap();
    assert!(!owed.needs_repropagate, "the owed change was sent");
}
