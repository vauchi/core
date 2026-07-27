// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression coverage for per-origin-device version floors on genesis-sealed
//! card deltas.

use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::exchange::genesis::GenesisEnvelope;
use vauchi_core::identity::{Identity, RegistryBroadcast};
use vauchi_core::network::mailbox_token::current_day_epoch;
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
