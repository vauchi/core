// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Verifies the device-link ratchet-state sync contract: a replacement
//! device that joins via the production device-link dance receives both
//! the contact and the ratchet state, so it can decrypt card updates
//! from existing contacts without an in-person re-exchange.
//!
//! Related problem records:
//! - `2026-07-10-multi-device-ratchet-topology-gap` (sequential
//!   replacement Goal 2)
//! - `2026-03-23-device-replacement-flow`

use crate::common::card_update::seal_update;
use crate::common::helpers::{
    assert_card_update_round_trips, create_vauchi_with_card, create_vauchi_with_identity,
    setup_ratchets,
};
use vauchi_core::api::Vauchi;
use vauchi_core::api::sync::process_single_card_update;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkResponder, ProximityProof,
};
use vauchi_core::identity::{DeviceRegistry, Identity};

fn now() -> u64 {
    vauchi_core::clock::SystemClock::shared().unix_seconds()
}

/// Joins a second device to `device1`'s identity via the full
/// initiator⇄responder link dance, syncing all of `device1`'s contacts,
/// own card, and ratchet states — the production replacement-flow payload.
fn join_second_device(device1: &Vauchi, device_name: &str) -> Vauchi {
    let identity = device1.identity().expect("device 1 has an identity");
    let master_seed = *identity.master_seed();
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&master_seed),
        identity.signing_keypair(),
    );
    let initiator = DeviceLinkInitiator::new(master_seed, identity, registry.clone(), now());

    let qr = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(qr, device_name.into(), now()).unwrap();
    let encrypted_request = responder.create_request(now()).unwrap();
    let (_confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();
    let proof = ProximityProof::Ultrasonic {
        challenge_response: initiator.proximity_challenge(),
        verified_at: now(),
    };

    // Build the sync payload through the orchestrator so it includes ratchet
    // states, matching the production replacement path.
    let current_device = identity.create_device_info(now());
    let orchestrator = vauchi_core::api::sync::DeviceSyncOrchestrator::new(
        device1.storage(),
        current_device,
        registry,
    );
    let sync_payload = orchestrator.create_full_sync_payload().unwrap();
    let sync_json = serde_json::to_string(&sync_payload).unwrap();

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
fn linked_device_receives_contact_and_ratchet_state() {
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
            .is_some(),
        "device-link sync must carry ratchet state for existing contacts"
    );
}

// @internal
#[test]
fn linked_device_decrypts_update_from_existing_contact() {
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

    // The joined device must decrypt the update now that the ratchet state
    // travelled with the contact via device-link sync.
    process_single_card_update(
        alice2.identity().unwrap(),
        alice2.storage(),
        &bob_id,
        &encrypted,
    )
    .expect("joined device must decrypt existing-contact updates after ratchet sync");
}

// @internal
#[test]
fn linked_device_sends_update_to_existing_contact() {
    let alice1 = create_vauchi_with_identity("Alice");
    alice1
        .add_own_field(ContactField::new(
            FieldType::Email,
            "email",
            "alice@old.com",
            0,
        ))
        .unwrap();
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "email", "bob@old.com")]);
    let shared = SymmetricKey::generate();

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
        alice1.own_card().unwrap().expect("Alice has an own card"),
        shared.clone(),
        0,
    );
    let alice_id_at_bob = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Bob sends first so he is the initiator; Alice's receive chain bootstraps
    // her send chain. Transfer that advanced state to the joined device.
    let (bob_ratchet, alice_ratchet) = setup_ratchets(&shared);
    alice1
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_id, &alice_ratchet, false)
        .unwrap();
    bob.save_ratchet_state(&alice_id_at_bob, &bob_ratchet)
        .unwrap();
    assert_card_update_round_trips(&bob, &alice1, &alice_id_at_bob, &bob_id);

    let alice2 = join_second_device(&alice1, "Alice Phone 2");

    // Alice2 edits her card and sends to Bob. This exercises the send path:
    // the joined device must have both the ratchet state and the responder role.
    assert_card_update_round_trips(&alice2, &bob, &bob_id, &alice_id_at_bob);
}

// @internal
// Spike: proves the core assumption that a ratchet state moved from one device
// to another can decrypt a subsequent contact update. If this fails, the
// ratchet-state-sync plan is wrong and we must pivot to per-device sessions.
#[test]
fn spike_transferred_ratchet_state_decrypts_on_fresh_device() {
    let alice1 = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "email", "bob@old.com")]);
    let shared = SymmetricKey::generate();

    // Alice stores Bob's real card (as an in-person exchange delivers it).
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

    // Bob sends first, so Bob is the initiator; Alice is the responder.
    let (bob_ratchet, alice_ratchet) = setup_ratchets(&shared);
    alice1
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_id, &alice_ratchet, false)
        .unwrap();
    bob.save_ratchet_state(&alice_id_at_bob, &bob_ratchet)
        .unwrap();

    // Advance the ratchet past bootstrap: Bob sends one update to Alice1.
    assert_card_update_round_trips(&bob, &alice1, &alice_id_at_bob, &bob_id);

    // Extract Alice's now-advanced ratchet state for Bob.
    let (alice_advanced_ratchet, is_initiator) = alice1
        .storage()
        .ratchets()
        .load_ratchet_state(&bob_id)
        .expect("ratchet load")
        .expect("ratchet exists");
    assert!(!is_initiator, "Alice must be the responder");

    // Create a "replacement" Alice device with the SAME identity (master seed).
    // This mirrors what device-link adoption does; without it the card-update
    // signature bound to Alice's identity key would fail.
    let mut alice2 = Vauchi::in_memory().unwrap();
    let alice2_identity = Identity::from_device_link(
        *alice1.identity().unwrap().master_seed(),
        "Alice".to_string(),
        1,
        "Alice Phone 2".to_string(),
        now(),
    );
    alice2.set_identity(alice2_identity).unwrap();

    let bob_contact_at_alice2 = Contact::from_exchange(
        *bob.identity().unwrap().signing_public_key(),
        bob.own_card().unwrap().expect("Bob has an own card"),
        shared.clone(),
        0,
    );
    alice2.add_contact(bob_contact_at_alice2).unwrap();
    alice2
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_id, &alice_advanced_ratchet, is_initiator)
        .unwrap();

    // Bob edits his card again and seals for Alice — any of Alice's devices
    // should be able to open it if the transferred state is sufficient.
    let bob_old = bob.own_card().unwrap().unwrap();
    let field_id = bob_old.fields().first().unwrap().id().to_string();
    let mut bob_new = bob_old.clone();
    bob_new
        .update_field_value(&field_id, "bob@spiked.com", 2)
        .unwrap();
    bob.update_own_card(&bob_new).unwrap();
    let encrypted = seal_update(&bob, &alice_id_at_bob, &bob_old, &bob_new, 2);

    let result = process_single_card_update(
        alice2.identity().unwrap(),
        alice2.storage(),
        &bob_id,
        &encrypted,
    );
    assert!(
        result.is_ok(),
        "transferred ratchet state must decrypt on the replacement device: {result:?}"
    );
}
