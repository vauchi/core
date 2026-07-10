// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Characterizes the multi-device ratchet topology gap (problem record
//! `2026-07-10-multi-device-ratchet-topology-gap`): the device-link
//! sync payload carries contacts + shared keys but neither ratchet
//! state nor the exchange ephemerals `bootstrap_exchange_ratchet`
//! requires, so a joined device cannot decrypt ratcheted updates from
//! existing contacts until a fresh re-exchange. When the topology
//! decision lands, the negative assertions here flip into the
//! resume-works contract.

use crate::common::card_update::seal_update;
use crate::common::helpers::{
    assert_card_update_round_trips, create_vauchi_with_card, create_vauchi_with_identity,
    setup_ratchets,
};
use vauchi_core::api::Vauchi;
use vauchi_core::api::sync::{CardUpdateError, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkResponder, ProximityProof,
};
use vauchi_core::identity::DeviceRegistry;
use vauchi_core::sync::DeviceSyncPayload;

fn now() -> u64 {
    vauchi_core::clock::SystemClock::shared().unix_seconds()
}

/// Joins a second device to `device1`'s identity via the full
/// initiator⇄responder link dance, syncing all of `device1`'s contacts
/// and own card — the production replacement-flow payload.
fn join_second_device(device1: &Vauchi, device_name: &str) -> Vauchi {
    let identity = device1.identity().expect("device 1 has an identity");
    let master_seed = *identity.master_seed();
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&master_seed),
        identity.signing_keypair(),
    );
    let initiator = DeviceLinkInitiator::new(master_seed, identity, registry, now());

    let qr = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(qr, device_name.into(), now()).unwrap();
    let encrypted_request = responder.create_request(now()).unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();
    let proof = ProximityProof::Ultrasonic {
        challenge_response: initiator.proximity_challenge(),
        verified_at: now(),
    };

    let contacts = device1.storage().contacts().list_contacts().unwrap();
    let own_card = device1
        .own_card()
        .unwrap()
        .unwrap_or_else(|| ContactCard::new("Alice"));
    let sync_json =
        serde_json::to_string(&DeviceSyncPayload::new(&contacts, &own_card, 1)).unwrap();
    let (encrypted_response, _registry, _new_device) = initiator
        .confirm_link_with_sync(&request, &sync_json, &proof, now())
        .unwrap();
    let response = responder.process_response(&encrypted_response).unwrap();

    let mut joiner = Vauchi::in_memory().unwrap();
    joiner
        .adopt_device_link_response(&response, device_name.into())
        .expect("adopt succeeds on a fresh instance");
    joiner
}

// @internal
#[test]
fn linked_device_receives_contact_but_no_ratchet_state() {
    let alice1 = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "email", "bob@old.com")]);
    let shared = SymmetricKey::generate();

    // Alice stores Bob's real card (as an in-person exchange delivers
    // it) so Bob's later field-edit delta has a base to apply to.
    let bob_contact = Contact::from_exchange(
        *bob.identity().unwrap().signing_public_key(),
        bob.own_card().unwrap().expect("Bob has an own card"),
        shared.clone(),
        0,
    );
    let bob_id = bob_contact.id().to_string();
    alice1.add_contact(bob_contact).unwrap();

    let alice_contact = Contact::from_exchange(
        *alice1.identity().unwrap().signing_public_key(),
        ContactCard::new("Alice"),
        shared.clone(),
        0,
    );
    let alice_id_at_bob = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Bob sends first, so Bob takes the initiator slot — a responder
    // ratchet has no sending chain until it has received once.
    let (bob_ratchet, alice_ratchet) = setup_ratchets(&shared);
    alice1.save_ratchet_state(&bob_id, &alice_ratchet).unwrap();
    bob.save_ratchet_state(&alice_id_at_bob, &bob_ratchet)
        .unwrap();

    // Positive control: the exchange is genuinely live — device 1
    // decrypts Bob's first update, advancing Bob's send ratchet past
    // the state any fresh bootstrap could reproduce.
    assert_card_update_round_trips(&bob, &alice1, &alice_id_at_bob, &bob_id);

    let alice2 = join_second_device(&alice1, "Alice Phone 2");

    let bob_at_alice2 = alice2
        .get_contact(&bob_id)
        .expect("contact query on the joined device");
    assert!(
        bob_at_alice2.is_some(),
        "the sync payload must deliver Bob to the joined device"
    );

    assert!(
        alice2
            .storage()
            .ratchets()
            .load_ratchet_state(&bob_id)
            .expect("ratchet store query on the joined device")
            .is_none(),
        "documents the gap: device-link sync carries no ratchet state for existing contacts"
    );
}

// @internal
#[test]
fn linked_device_cannot_decrypt_update_from_existing_contact() {
    let alice1 = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "email", "bob@old.com")]);
    let shared = SymmetricKey::generate();

    // Alice stores Bob's real card (as an in-person exchange delivers
    // it) so Bob's later field-edit delta has a base to apply to.
    let bob_contact = Contact::from_exchange(
        *bob.identity().unwrap().signing_public_key(),
        bob.own_card().unwrap().expect("Bob has an own card"),
        shared.clone(),
        0,
    );
    let bob_id = bob_contact.id().to_string();
    alice1.add_contact(bob_contact).unwrap();

    let alice_contact = Contact::from_exchange(
        *alice1.identity().unwrap().signing_public_key(),
        ContactCard::new("Alice"),
        shared.clone(),
        0,
    );
    let alice_id_at_bob = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Bob sends first, so Bob takes the initiator slot — a responder
    // ratchet has no sending chain until it has received once.
    let (bob_ratchet, alice_ratchet) = setup_ratchets(&shared);
    alice1.save_ratchet_state(&bob_id, &alice_ratchet).unwrap();
    bob.save_ratchet_state(&alice_id_at_bob, &bob_ratchet)
        .unwrap();
    assert_card_update_round_trips(&bob, &alice1, &alice_id_at_bob, &bob_id);

    let alice2 = join_second_device(&alice1, "Alice Phone 2");

    // Bob edits his card again and seals for Alice — the update any of
    // Alice's devices should be able to open.
    let bob_old = bob.own_card().unwrap().unwrap();
    let field_id = bob_old.fields().first().unwrap().id().to_string();
    let mut bob_new = bob_old.clone();
    bob_new
        .update_field_value(&field_id, "bob@newer.com", 2)
        .unwrap();
    bob.update_own_card(&bob_new).unwrap();
    let encrypted = seal_update(&bob, &alice_id_at_bob, &bob_old, &bob_new, 2);

    // NoRatchetState (not ContactNotFound) proves the contact synced
    // fine and the ratchet alone is what's missing.
    let err = process_single_card_update(
        alice2.identity().unwrap(),
        alice2.storage(),
        &bob_id,
        &encrypted,
    )
    .expect_err("documents the gap: the joined device cannot decrypt existing-contact updates");
    assert!(
        matches!(err, CardUpdateError::NoRatchetState),
        "expected NoRatchetState, got {err:?}"
    );
}
