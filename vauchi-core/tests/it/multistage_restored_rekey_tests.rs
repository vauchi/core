// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact restored from a guardian backup adopts the key of an
//! in-person exchange only once the peer said it saved too
//! (vauchi/private#1217, Tamarin `multistage_exchange.spthy`). Adopting a
//! key the peer may never store would cut the only channel left.

use vauchi_core::SymmetricKey;
use vauchi_core::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::signing_pk;
use crate::common::recovered_phone::{World, recovered};

fn restored() -> World {
    let world = recovered(create_vauchi_with_identity("Alice, new phone"), None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    world
}

/// Dave as the phone's exchange with him in person shows him, with a new
/// shared key and the role's ratchet.
fn dave_met_in_person(world: &World) -> (Contact, DoubleRatchetState) {
    let shared = SymmetricKey::generate();
    let dave = Contact::from_exchange(
        signing_pk(world.dave.vauchi()),
        ContactCard::new("Dave, in person"),
        shared.clone(),
        0,
    );
    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared, *X3DHKeyPair::generate().public_key())
            .unwrap();
    (dave, ratchet)
}

fn shared_key_of(phone: &Vauchi, id: &str) -> Option<[u8; 32]> {
    phone
        .get_contact(id)
        .unwrap()
        .and_then(|c| c.shared_key().map(|k| *k.as_bytes()))
}

// @internal
#[test]
fn before_the_peer_saved_a_restored_contact_gets_the_card_but_keeps_its_key_and_waits() {
    let world = restored();
    let old_key = shared_key_of(&world.phone, &world.dave_id);
    let (dave, ratchet) = dave_met_in_person(&world);
    assert_eq!(dave.id(), world.dave_id);

    let awaiting = world
        .phone
        .save_exchanged_contact_before_peer_saved(&dave, &ratchet, true)
        .unwrap();

    assert!(awaiting);
    assert_eq!(shared_key_of(&world.phone, &world.dave_id), old_key);
    assert_eq!(
        world
            .phone
            .get_contact(&world.dave_id)
            .unwrap()
            .unwrap()
            .display_name(),
        "Dave, in person"
    );
    assert!(
        world
            .phone
            .rekey_pending_contacts()
            .unwrap()
            .contains(&world.dave_id)
    );
}

// @internal
#[test]
fn once_the_peer_saved_the_restored_contact_adopts_the_exchange_key() {
    let world = restored();
    let (dave, ratchet) = dave_met_in_person(&world);
    world
        .phone
        .save_exchanged_contact_before_peer_saved(&dave, &ratchet, true)
        .unwrap();

    world
        .phone
        .save_exchanged_contact(&dave, &ratchet, true)
        .unwrap();

    assert_eq!(
        shared_key_of(&world.phone, &world.dave_id),
        dave.shared_key().map(|k| *k.as_bytes())
    );
    assert_eq!(
        world
            .phone
            .save_exchanged_contact_before_peer_saved(&dave, &ratchet, true)
            .unwrap(),
        false,
        "met again: nothing waits for the peer any more"
    );
}

// @internal
#[test]
fn a_contact_not_waiting_for_a_rekey_is_saved_whole_before_the_peer_saved() {
    let phone = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let shared = SymmetricKey::generate();
    let contact =
        Contact::from_exchange(signing_pk(&bob), ContactCard::new("Bob"), shared.clone(), 0);
    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared, *X3DHKeyPair::generate().public_key())
            .unwrap();

    let awaiting = phone
        .save_exchanged_contact_before_peer_saved(&contact, &ratchet, true)
        .unwrap();

    assert!(!awaiting);
    assert_eq!(
        shared_key_of(&phone, contact.id()),
        Some(*shared.as_bytes())
    );
}
