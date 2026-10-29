// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use vauchi_core::sync::delta::FieldChange;

fn wrapped(delta: &CardDelta) -> Vec<u8> {
    let cek = ContentEncryptionKey::generate();
    VersionedPayload::encode_cek(&CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext: cek.encrypt(&serde_json::to_vec(delta).unwrap()).unwrap(),
        signature: delta.signature,
        nonce: delta.nonce,
    })
}

fn ordinary_blob(world: &ColdStartWorld, sender: &Identity, mut delta: CardDelta) -> Vec<u8> {
    let receiver_dh = X3DHKeyPair::generate();
    let mut sending =
        DoubleRatchetState::initialize_initiator(&world.shared_key, *receiver_dh.public_key())
            .unwrap();
    let receiving = DoubleRatchetState::initialize_responder(&world.shared_key, receiver_dh);
    world
        .bob
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(
            &world.alice_contact_id,
            sender.device_id(),
            &receiving,
            false,
        )
        .unwrap();
    delta.sign(sender, world.bob.identity().unwrap().signing_public_key());
    serde_json::to_vec(&sending.encrypt(&wrapped(&delta)).unwrap()).unwrap()
}

fn receive(
    world: &ColdStartWorld,
    sender: &Identity,
    blob: &[u8],
    ordinary: bool,
) -> Result<ReceiveOutcome, CardUpdateError> {
    if ordinary {
        process_single_card_update_for_device(
            world.bob.identity().unwrap(),
            world.bob.storage(),
            &world.alice_contact_id,
            sender.device_id(),
            blob,
        )
    } else {
        process_single_card_update(
            world.bob.identity().unwrap(),
            world.bob.storage(),
            &world.alice_contact_id,
            blob,
        )
    }
}

fn blob(world: &ColdStartWorld, sender: &Identity, delta: CardDelta, ordinary: bool) -> Vec<u8> {
    if ordinary {
        ordinary_blob(world, sender, delta)
    } else {
        seal_delta_for_world(world, sender, delta)
    }
}

// @claim: S2
// @scenario: sync_updates :: A multi-device sender's versions are floored per device
#[test]
fn peer_card_ordering_blocks_held_removal_and_name_on_both_receive_paths() {
    for ordinary in [false, true] {
        let world = cold_start_world();
        let mut older = world.base_card.clone();
        older
            .remove_field(world.base_card.fields()[0].id())
            .unwrap();
        older.set_display_name("Earlier").unwrap();
        let old = blob(
            &world,
            &world.alice_a1,
            CardDelta::compute(&world.base_card, &older, 10),
            ordinary,
        );
        let mut newer = world.base_card.clone();
        newer
            .update_field_value(world.base_card.fields()[0].id(), "latest@example.com", 20)
            .unwrap();
        newer.set_display_name("Latest").unwrap();
        let new = blob(
            &world,
            &world.alice_a2,
            CardDelta::compute(&world.base_card, &newer, 20),
            ordinary,
        );
        receive(&world, &world.alice_a2, &new, ordinary).unwrap();
        receive(&world, &world.alice_a1, &old, ordinary).unwrap();
        assert_eq!(
            world
                .bob
                .get_contact(&world.alice_contact_id)
                .unwrap()
                .unwrap()
                .card()
                .fields()
                .len(),
            1,
            "held removal must not erase newer field"
        );
        assert_eq!(stored_email_value(&world), "latest@example.com");
        assert_eq!(
            world
                .bob
                .get_contact(&world.alice_contact_id)
                .unwrap()
                .unwrap()
                .display_name(),
            "Latest"
        );
    }
}

// @claim: S2
// @scenario: sync_updates :: A multi-device sender's versions are floored per device
#[test]
fn peer_card_ordering_blocks_held_add_on_both_receive_paths() {
    for ordinary in [false, true] {
        let world = cold_start_world();
        let mut older = world.base_card.clone();
        older
            .update_field_value(world.base_card.fields()[0].id(), "old@example.com", 10)
            .unwrap();
        let old = blob(
            &world,
            &world.alice_a1,
            CardDelta::compute(&world.base_card, &older, 10),
            ordinary,
        );
        let mut newer = world.base_card.clone();
        newer
            .remove_field(world.base_card.fields()[0].id())
            .unwrap();
        let new = blob(
            &world,
            &world.alice_a2,
            CardDelta::compute(&world.base_card, &newer, 20),
            ordinary,
        );
        receive(&world, &world.alice_a2, &new, ordinary).unwrap();
        receive(&world, &world.alice_a1, &old, ordinary).unwrap();
        assert!(
            world
                .bob
                .get_contact(&world.alice_contact_id)
                .unwrap()
                .unwrap()
                .card()
                .fields()
                .is_empty()
        );
    }
}

// @claim: S2
// @scenario: sync_updates :: Genesis card versions are floored per origin device
#[test]
fn peer_card_ordering_allows_same_second_successor_and_newer_readd() {
    let world = cold_start_world();
    let mut removed = world.base_card.clone();
    removed
        .remove_field(world.base_card.fields()[0].id())
        .unwrap();
    removed.set_display_name("Removed").unwrap();
    let mut first = CardDelta::compute(&world.base_card, &removed, 20);
    first.set_version(1);
    let first = seal_delta_for_world(&world, &world.alice_a1, first);
    receive(&world, &world.alice_a1, &first, false).unwrap();
    let mut restored = world.base_card.clone();
    restored
        .update_field_value(world.base_card.fields()[0].id(), "restored@example.com", 20)
        .unwrap();
    restored.set_display_name("Restored").unwrap();
    let mut second = CardDelta::compute(&removed, &restored, 20);
    second.set_version(2);
    let second = seal_delta_for_world(&world, &world.alice_a1, second);
    receive(&world, &world.alice_a1, &second, false).unwrap();
    assert_eq!(stored_email_value(&world), "restored@example.com");
    assert_eq!(
        world
            .bob
            .get_contact(&world.alice_contact_id)
            .unwrap()
            .unwrap()
            .display_name(),
        "Restored"
    );
}

// @claim: S2
// @scenario: sync_updates :: Genesis card versions are floored per origin device
#[test]
fn peer_card_ordering_survives_receiver_reload() {
    let directory = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let config =
        vauchi_core::api::VauchiConfig::with_storage_path(directory.path().join("receive.db"))
            .with_storage_key(key);
    let mut world = cold_start_world();
    let contact = world
        .bob
        .get_contact(&world.alice_contact_id)
        .unwrap()
        .unwrap();
    let mut bob = vauchi_core::Vauchi::new(config.clone()).unwrap();
    bob.create_identity("Bob").unwrap();
    bob.add_contact(contact).unwrap();
    world.bob = bob;
    let mut old_card = world.base_card.clone();
    old_card
        .update_field_value(world.base_card.fields()[0].id(), "held@example.com", 10)
        .unwrap();
    let held = seal_delta_for_world(
        &world,
        &world.alice_a1,
        CardDelta::compute(&world.base_card, &old_card, 10),
    );
    let mut removed = world.base_card.clone();
    removed
        .remove_field(world.base_card.fields()[0].id())
        .unwrap();
    let removal = seal_delta_for_world(
        &world,
        &world.alice_a2,
        CardDelta::compute(&world.base_card, &removed, 20),
    );
    receive(&world, &world.alice_a2, &removal, false).unwrap();
    drop(world.bob);
    world.bob = vauchi_core::Vauchi::new(config).unwrap();
    receive(&world, &world.alice_a1, &held, false).unwrap();
    assert!(
        world
            .bob
            .get_contact(&world.alice_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .is_empty()
    );
}

// @claim: S2
// @scenario: sync_updates :: Genesis card versions are floored per origin device
#[test]
fn peer_card_ordering_capacity_rejection_leaves_no_receive_state() {
    for ordinary in [false, true] {
        let world = cold_start_world();
        for start in (0..4095).step_by(256) {
            let mut filling = CardDelta::compute(&world.base_card, &world.base_card, 20);
            filling.set_version((start / 256 + 1) as u32);
            filling.changes = (start..(start + 256).min(4095))
                .map(|i| FieldChange::Removed {
                    field_id: format!("deleted-{i}"),
                })
                .collect();
            let ciphertext = blob(&world, &world.alice_a1, filling, true);
            receive(&world, &world.alice_a1, &ciphertext, true).unwrap();
        }
        // Repeated removals occupy one key, and a full map still permits a
        // legitimate edit to a field it already tracks.
        let mut edited = world.base_card.clone();
        edited
            .update_field_value(world.base_card.fields()[0].id(), "at-cap@example.com", 30)
            .unwrap();
        let mut existing = CardDelta::compute(&world.base_card, &edited, 30);
        existing.set_version(17);
        existing.changes.extend([
            FieldChange::Removed {
                field_id: "deleted-0".into(),
            },
            FieldChange::Removed {
                field_id: "deleted-0".into(),
            },
        ]);
        let ciphertext = blob(&world, &world.alice_a1, existing, ordinary);
        receive(&world, &world.alice_a1, &ciphertext, ordinary).unwrap();
        assert_eq!(stored_email_value(&world), "at-cap@example.com");
        let before = world
            .bob
            .storage()
            .contacts()
            .test_load_card_ordering(&world.alice_contact_id)
            .unwrap();
        let mut delta = CardDelta::compute(&world.base_card, &world.base_card, 20);
        delta.set_version(18);
        delta.changes = vec![FieldChange::Removed {
            field_id: "capacity-overflow".into(),
        }];
        let nonce = delta.nonce;
        let ciphertext = blob(&world, &world.alice_a1, delta, ordinary);
        let error = receive(&world, &world.alice_a1, &ciphertext, ordinary)
            .expect_err("ordering capacity must fail closed");
        assert!(format!("{error:?}").contains("OrderingCapacityExceeded"));
        assert!(
            !world
                .bob
                .storage()
                .replay()
                .is_replay_nonce(&world.alice_contact_id, &nonce)
                .unwrap()
        );
        assert_eq!(
            world
                .bob
                .storage()
                .contacts()
                .test_load_card_ordering(&world.alice_contact_id)
                .unwrap(),
            before
        );
        assert_eq!(stored_email_value(&world), "at-cap@example.com");
    }
}

// @claim: S2
// @scenario: sync_updates :: Genesis card versions are floored per origin device
#[test]
fn peer_card_ordering_invalid_signature_writes_no_stamps() {
    let world = cold_start_world();
    let mut changed = world.base_card.clone();
    changed
        .remove_field(world.base_card.fields()[0].id())
        .unwrap();
    let mut delta = CardDelta::compute(&world.base_card, &changed, 20);
    delta.sign(
        &world.alice_a1,
        world.bob.identity().unwrap().signing_public_key(),
    );
    delta.signature[0] ^= 1;
    let (message, _) = GenesisEnvelope::seal(
        &world.shared_key,
        &world.alice_a1,
        world.bob.identity().unwrap().signing_public_key(),
        &world.registry,
        current_day_epoch(20),
        &wrapped(&delta),
    )
    .unwrap();
    let error = receive(
        &world,
        &world.alice_a1,
        &serde_json::to_vec(&message).unwrap(),
        false,
    )
    .unwrap_err();
    assert!(matches!(error, CardUpdateError::SignatureInvalid));
    assert!(
        world
            .bob
            .storage()
            .contacts()
            .test_load_card_ordering(&world.alice_contact_id)
            .unwrap()
            .is_none()
    );
    assert!(
        !world
            .bob
            .storage()
            .replay()
            .is_replay_nonce(&world.alice_contact_id, &delta.nonce)
            .unwrap()
    );
}

// @claim: S2
// @scenario: sync_updates :: Genesis card versions are floored per origin device
#[test]
fn peer_card_ordering_write_failure_rolls_back_card_nonce_and_ratchet() {
    for ordinary in [false, true] {
        let world = cold_start_world();
        let mut changed = world.base_card.clone();
        changed
            .remove_field(world.base_card.fields()[0].id())
            .unwrap();
        let delta = CardDelta::compute(&world.base_card, &changed, 20);
        let nonce = delta.nonce;
        let ciphertext = blob(&world, &world.alice_a1, delta, ordinary);
        world.bob.storage().connection().execute_batch("CREATE TRIGGER reject_ordering BEFORE INSERT ON peer_card_ordering BEGIN SELECT RAISE(ABORT, 'ordering write fault'); END;").unwrap();
        let error = receive(&world, &world.alice_a1, &ciphertext, ordinary)
            .expect_err("ordering write must be atomic");
        assert!(matches!(error, CardUpdateError::Storage(_)));
        assert_eq!(stored_email_value(&world), "before@example.com");
        assert!(
            !world
                .bob
                .storage()
                .replay()
                .is_replay_nonce(&world.alice_contact_id, &nonce)
                .unwrap()
        );
        assert!(
            world
                .bob
                .storage()
                .contacts()
                .test_load_card_ordering(&world.alice_contact_id)
                .unwrap()
                .is_none()
        );
        world
            .bob
            .storage()
            .connection()
            .execute_batch("DROP TRIGGER reject_ordering;")
            .unwrap();
        receive(&world, &world.alice_a1, &ciphertext, ordinary)
            .expect("rollback must preserve decryptability for retry");
        assert!(
            world
                .bob
                .get_contact(&world.alice_contact_id)
                .unwrap()
                .unwrap()
                .card()
                .fields()
                .is_empty()
        );
    }
}
