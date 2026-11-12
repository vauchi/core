// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use vauchi_core::exchange::X3DHKeyPair;

// @scenario: multi_device_sync :: Card update propagation across devices
// @internal
#[test]
fn waiting_for_one_peer_does_not_repeat_updates_to_a_ready_peer() {
    let world = pending_activation();
    let carol = create_vauchi_with_identity("Carol");
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *carol.identity().unwrap().signing_public_key(),
        ContactCard::new("Carol"),
        shared.clone(),
        0,
    );
    let carol_id = contact.id().to_string();
    world.alice.add_contact(contact).unwrap();
    world
        .alice
        .create_ratchet_as_initiator(&carol_id, &shared, *X3DHKeyPair::generate().public_key())
        .unwrap();
    world
        .alice
        .set_contact_visibility_override(&carol_id, &world.field_id, true)
        .unwrap();

    wait_for_activation(&world);

    assert_eq!(
        world
            .alice
            .storage()
            .pending()
            .get_pending_updates(&carol_id)
            .unwrap()
            .len(),
        1,
        "a pending peer must not cause repeated full-card updates to Carol"
    );
}

fn reject_marker_write(world: &PendingActivation) {
    world
        .alice
        .storage()
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_owed BEFORE UPDATE OF own_card_repropagate_encrypted
         ON ux_state BEGIN SELECT RAISE(ABORT, 'injected marker write failure'); END;",
        )
        .unwrap();
}

fn allow_marker_write(world: &PendingActivation) {
    world
        .alice
        .storage()
        .connection()
        .execute_batch("DROP TRIGGER reject_owed")
        .unwrap();
}

// @scenario: multi_device_sync :: Siblings adopt relayed activation state
// @internal
#[test]
fn failed_sibling_marker_write_rolls_back_activation_and_can_be_replayed() {
    let world = pending_activation();
    wait_for_activation(&world);
    reject_marker_write(&world);

    let applied = world
        .alice
        .apply_sync_items(vec![activation_item(&world)])
        .unwrap();

    assert_eq!(applied, 0);
    assert!(
        world
            .alice
            .storage()
            .registry_activation()
            .load_activation(&world.bob_at_alice)
            .unwrap()
            .is_none()
    );
    allow_marker_write(&world);
    assert_eq!(
        world
            .alice
            .apply_sync_items(vec![activation_item(&world)])
            .unwrap(),
        1
    );
    assert_owed_field_delivered(&world);
}

// @scenario: multi_device_sync :: A joining device inherits activation state via full sync
// @internal
#[test]
fn failed_full_sync_marker_write_rolls_back_activation_and_can_be_replayed() {
    let world = pending_activation();
    wait_for_activation(&world);
    reject_marker_write(&world);

    let result = apply_full_activation(&world);

    assert!(
        matches!(result, Err(DeviceSyncError::Serialization(_))),
        "{result:?}"
    );
    assert!(
        world
            .alice
            .storage()
            .registry_activation()
            .load_activation(&world.bob_at_alice)
            .unwrap()
            .is_none()
    );
    allow_marker_write(&world);
    apply_full_activation(&world).unwrap();
    assert_owed_field_delivered(&world);
}
