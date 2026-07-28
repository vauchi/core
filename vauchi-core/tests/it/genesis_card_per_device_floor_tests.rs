// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression coverage for per-origin-device version floors on genesis-sealed
//! card deltas.

use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::sync::card_update::process_single_card_update_for_authenticated_device;
use vauchi_core::api::{
    CardUpdateError, ReceiveOutcome, process_single_card_update,
    process_single_card_update_for_device,
};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::crypto::x3dh::X3DHKeyPair;
use vauchi_core::exchange::genesis::GenesisEnvelope;
use vauchi_core::identity::{Identity, RegistryBroadcast};
use vauchi_core::network::mailbox_token::current_day_epoch;
use vauchi_core::storage::GENESIS_CONTACT_ATTEMPTS_PER_WINDOW;
use vauchi_core::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

struct ColdStartWorld {
    alice_a1: Identity,
    alice_a2: Identity,
    bob: vauchi_core::Vauchi,
    alice_contact_id: String,
    shared_key: SymmetricKey,
    base_card: ContactCard,
    registry: RegistryBroadcast,
}

fn cold_start_world() -> ColdStartWorld {
    let alice_a1 = Identity::create("Alice", 0);
    let alice_a2 = Identity::from_device_link(
        *alice_a1.master_seed(),
        "Alice".into(),
        1,
        "Alice A2".into(),
        1,
    );
    let mut registry = alice_a1.initial_device_registry();
    registry
        .add_device(
            alice_a2.device_info().to_registered(alice_a1.master_seed()),
            alice_a1.signing_keypair(),
        )
        .unwrap();
    let registry = RegistryBroadcast::new(&registry, alice_a1.signing_keypair(), 2);

    let bob = create_vauchi_with_identity("Bob");
    let shared_key = SymmetricKey::generate();
    let mut base_card = ContactCard::new("Alice");
    base_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "before@example.com",
            0,
        ))
        .unwrap();
    let alice_contact = Contact::from_exchange(
        *alice_a1.signing_public_key(),
        base_card.clone(),
        shared_key.clone(),
        0,
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    ColdStartWorld {
        alice_a1,
        alice_a2,
        bob,
        alice_contact_id,
        shared_key,
        base_card,
        registry,
    }
}

fn genesis_card_blob(
    sender: &Identity,
    bob_pk: &[u8; 32],
    shared_key: &SymmetricKey,
    registry: &RegistryBroadcast,
    base_card: &ContactCard,
    value: &str,
    version: u32,
    timestamp: u64,
) -> Vec<u8> {
    let mut edited_card = base_card.clone();
    edited_card
        .update_field_value(base_card.fields()[0].id(), value, timestamp)
        .unwrap();

    let mut delta = CardDelta::compute(base_card, &edited_card, timestamp);
    delta.set_version(version);
    delta.sign(sender, bob_pk);

    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek = ContentEncryptionKey::generate();
    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext: cek.encrypt(&delta_bytes).unwrap(),
        signature: delta.signature,
        nonce: delta.nonce,
    };
    let payload = VersionedPayload::encode_cek(&wrapped);
    let (message, _) = GenesisEnvelope::seal(
        shared_key,
        sender,
        bob_pk,
        registry,
        current_day_epoch(timestamp),
        &payload,
    )
    .unwrap();
    serde_json::to_vec(&message).unwrap()
}

fn stored_email_value(world: &ColdStartWorld) -> String {
    world
        .bob
        .storage()
        .contacts()
        .load_contact(&world.alice_contact_id)
        .unwrap()
        .unwrap()
        .card()
        .fields()[0]
        .value()
        .to_string()
}

// @scenario: sync_updates :: Genesis card versions are floored per origin device
#[test]
fn genesis_card_per_device_floor_accepts_each_device_and_rejects_its_own_stale_version() {
    let world = cold_start_world();
    let bob_pk = *world.bob.identity().unwrap().signing_public_key();

    let a1_v2 = genesis_card_blob(
        &world.alice_a1,
        &bob_pk,
        &world.shared_key,
        &world.registry,
        &world.base_card,
        "a1-v2@example.com",
        2,
        10,
    );
    let a1_outcome = process_single_card_update(
        world.bob.identity().unwrap(),
        world.bob.storage(),
        &world.alice_contact_id,
        &a1_v2,
    )
    .expect("A1's genesis-sealed v2 card delta applies");
    assert!(matches!(a1_outcome, ReceiveOutcome::CardDelta));
    assert_eq!(stored_email_value(&world), "a1-v2@example.com");

    let a2_v1 = genesis_card_blob(
        &world.alice_a2,
        &bob_pk,
        &world.shared_key,
        &world.registry,
        &world.base_card,
        "a2-v1@example.com",
        1,
        20,
    );
    let a2_outcome = process_single_card_update(
        world.bob.identity().unwrap(),
        world.bob.storage(),
        &world.alice_contact_id,
        &a2_v1,
    )
    .expect("A2's genesis-sealed v1 uses an independent device floor");
    assert!(matches!(a2_outcome, ReceiveOutcome::CardDelta));
    assert_eq!(stored_email_value(&world), "a2-v1@example.com");

    let a1_v1 = genesis_card_blob(
        &world.alice_a1,
        &bob_pk,
        &world.shared_key,
        &world.registry,
        &world.base_card,
        "a1-stale@example.com",
        1,
        30,
    );
    let stale = process_single_card_update(
        world.bob.identity().unwrap(),
        world.bob.storage(),
        &world.alice_contact_id,
        &a1_v1,
    );
    assert!(matches!(
        stale,
        Err(CardUpdateError::StaleVersion { delta: 1, last: 2 })
    ));
    assert_eq!(stored_email_value(&world), "a2-v1@example.com");
}

// @scenario: multi_device_sync :: A responder's device-scoped genesis card fallback is received
#[test]
fn device_scoped_receive_opens_genesis_card_after_ratchet_decrypt_fails() {
    let world = cold_start_world();
    let bob_pk = *world.bob.identity().unwrap().signing_public_key();
    let alice_device_id = *world.alice_a1.device_id();

    // Active F4 delivery resolves the authenticated origin device before
    // decrypting. Keep an ordinary device-pair ratchet at that key so the
    // receive path takes the established device-scoped branch first.
    let peer_dh = X3DHKeyPair::generate();
    let established =
        DoubleRatchetState::initialize_initiator(&world.shared_key, *peer_dh.public_key()).unwrap();
    world
        .bob
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(
            &world.alice_contact_id,
            &alice_device_id,
            &established,
            true,
        )
        .unwrap();

    // A responder with no sending chain uses the production genesis fallback
    // while retaining the non-zero recipient-device route.
    let blob = genesis_card_blob(
        &world.alice_a1,
        &bob_pk,
        &world.shared_key,
        &world.registry,
        &world.base_card,
        "responder-update@example.com",
        1,
        10,
    );
    let outcome = process_single_card_update_for_authenticated_device(
        world.bob.identity().unwrap(),
        world.bob.storage(),
        &world.alice_contact_id,
        &alice_device_id,
        &blob,
    )
    .expect("device-scoped receive must open the responder's genesis card fallback");

    assert!(matches!(outcome, ReceiveOutcome::CardDelta));
    assert_eq!(stored_email_value(&world), "responder-update@example.com");
    let stored = world
        .bob
        .storage()
        .contacts()
        .load_contact(&world.alice_contact_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.card_updated_at(),
        Some(10),
        "genesis fallback must preserve the verified sender timestamp"
    );
    assert_eq!(
        world
            .bob
            .storage()
            .genesis_limits()
            .contact_attempts_in_window(&world.alice_contact_id)
            .unwrap(),
        0,
        "an origin-hint-authenticated card fallback must not consume the safety-alert budget"
    );
    assert!(
        world
            .bob
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&world.alice_contact_id, &alice_device_id)
            .unwrap()
            .is_some(),
        "a successful genesis fallback must not replace the device ratchet"
    );

    let replay = process_single_card_update_for_authenticated_device(
        world.bob.identity().unwrap(),
        world.bob.storage(),
        &world.alice_contact_id,
        &alice_device_id,
        &blob,
    );
    assert!(matches!(replay, Err(CardUpdateError::ReplayDetected)));
    assert!(
        world
            .bob
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&world.alice_contact_id, &alice_device_id)
            .unwrap()
            .is_some(),
        "a genesis replay rejection must not repair a healthy device ratchet"
    );
}

// @scenario: multi_device_sync :: Safety-alert rate limiting does not block a device card fallback
#[test]
fn safety_alert_budget_does_not_block_device_scoped_genesis_card() {
    let world = cold_start_world();
    let bob_pk = *world.bob.identity().unwrap().signing_public_key();
    let alice_device_id = *world.alice_a1.device_id();
    let peer_dh = X3DHKeyPair::generate();
    let established =
        DoubleRatchetState::initialize_initiator(&world.shared_key, *peer_dh.public_key()).unwrap();
    world
        .bob
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(
            &world.alice_contact_id,
            &alice_device_id,
            &established,
            true,
        )
        .unwrap();

    for _ in 0..GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
        assert!(
            world
                .bob
                .storage()
                .genesis_limits()
                .consume_decrypt_budget(&world.alice_contact_id)
                .unwrap()
        );
    }

    let blob = genesis_card_blob(
        &world.alice_a1,
        &bob_pk,
        &world.shared_key,
        &world.registry,
        &world.base_card,
        "rate-limited@example.com",
        1,
        10,
    );
    let untrusted = process_single_card_update_for_device(
        world.bob.identity().unwrap(),
        world.bob.storage(),
        &world.alice_contact_id,
        &alice_device_id,
        &blob,
    );
    assert!(
        matches!(untrusted, Err(CardUpdateError::DecryptionFailed)),
        "an over-budget speculative route must stay ACKable, got {untrusted:?}"
    );
    assert!(
        world
            .bob
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&world.alice_contact_id, &alice_device_id)
            .unwrap()
            .is_some(),
        "rate-limit rejection must not be mistaken for ratchet divergence"
    );

    let outcome = process_single_card_update_for_authenticated_device(
        world.bob.identity().unwrap(),
        world.bob.storage(),
        &world.alice_contact_id,
        &alice_device_id,
        &blob,
    )
    .expect("the authenticated device-card path has an independent admission boundary");

    assert!(
        matches!(outcome, ReceiveOutcome::CardDelta),
        "the exhausted safety-alert budget must not reject a card fallback"
    );
    assert_eq!(stored_email_value(&world), "rate-limited@example.com");
    assert!(
        world
            .bob
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&world.alice_contact_id, &alice_device_id)
            .unwrap()
            .is_some(),
        "the accepted card fallback must preserve the device session"
    );
}
