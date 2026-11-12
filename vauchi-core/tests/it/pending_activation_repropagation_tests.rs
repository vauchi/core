// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Visibility changes remain owed while a sibling awaits registry activation.

use crate::common::device_sync::device_signed_broadcast;
use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::api::process_single_card_update_for_device;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::sync::device_sync::ContactActivationSyncData;
use vauchi_core::sync::{DeviceSyncError, DeviceSyncPayload, SyncItem};
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi};

mod delivery_safety;

struct PendingActivation {
    alice: Vauchi,
    bob: Vauchi,
    bob_at_alice: String,
    alice_at_bob: String,
    field_id: String,
}

fn pending_activation() -> PendingActivation {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let shared = SymmetricKey::generate();
    let bob_contact = Contact::from_exchange(
        *bob.identity().unwrap().signing_public_key(),
        ContactCard::new("Bob"),
        shared.clone(),
        0,
    );
    let bob_at_alice = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();
    let alice_contact = Contact::from_exchange(
        *alice.identity().unwrap().signing_public_key(),
        ContactCard::new("Alice"),
        shared,
        0,
    );
    let alice_at_bob = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    for (receiver, peer, contact_id) in
        [(&alice, &bob, &bob_at_alice), (&bob, &alice, &alice_at_bob)]
    {
        receiver
            .storage()
            .device()
            .save_contact_device_registry(
                contact_id,
                &device_signed_broadcast(peer),
                peer.identity().unwrap().signing_public_key(),
                60,
            )
            .unwrap();
    }

    let field = ContactField::new(FieldType::Email, "work", "alice@example.com", 0);
    let field_id = field.id().to_string();
    alice.add_own_field(field).unwrap();
    alice
        .set_contact_visibility_override(&bob_at_alice, &field_id, false)
        .unwrap();
    alice.initialize_sent_baseline(&bob_at_alice).unwrap();
    alice
        .storage()
        .contacts()
        .save_last_sent_display_name(&bob_at_alice, "Alice")
        .unwrap();
    alice.run_owed_repropagation().unwrap();
    assert!(
        !alice
            .storage()
            .ux()
            .load_own_card_repropagate()
            .unwrap()
            .needs_repropagate
    );
    PendingActivation {
        alice,
        bob,
        bob_at_alice,
        alice_at_bob,
        field_id,
    }
}

fn mark_visible_and_owed(world: &PendingActivation) {
    world
        .alice
        .set_contact_visibility_override(&world.bob_at_alice, &world.field_id, true)
        .unwrap();
    world.alice.mark_own_card_repropagate().unwrap();
}

fn wait_for_activation(world: &PendingActivation) {
    mark_visible_and_owed(world);
    for _ in 0..8 {
        world.alice.run_owed_repropagation().unwrap();
    }
}

fn activation_item(world: &PendingActivation) -> SyncItem {
    SyncItem::ContactActivationChanged {
        contact_id: world.bob_at_alice.clone(),
        push_nonce: Some(vec![7u8; 32]),
        pushed_version: Some(1),
        our_version_acked: Some(1),
        peer_version_held: Some(1),
        timestamp: world.alice.storage().clock().unix_seconds(),
    }
}

fn apply_full_activation(world: &PendingActivation) -> Result<(), DeviceSyncError> {
    let identity = world.alice.identity().unwrap();
    let device = identity.create_device_info(world.alice.storage().clock().unix_seconds());
    let payload =
        DeviceSyncPayload::empty().with_contact_activations(vec![ContactActivationSyncData {
            contact_id: world.bob_at_alice.clone(),
            push_nonce: Some(vec![7u8; 32]),
            pushed_version: Some(1),
            our_version_acked: Some(1),
            peer_version_held: Some(1),
        }]);
    DeviceSyncOrchestrator::new(
        world.alice.storage(),
        device,
        identity.initial_device_registry(),
    )
    .apply_full_sync(payload)
}

// @scenario: visibility_control :: Granting visibility sends update to contact
// @internal
#[test]
fn visibility_grant_awaiting_activation_is_accepted_and_remembered() {
    let world = pending_activation();

    let result = world.alice.set_contact_visibility_override_and_repropagate(
        &world.bob_at_alice,
        &world.field_id,
        true,
    );

    assert!(
        matches!(result, Ok(())),
        "visibility grant failed: {result:?}"
    );
    let owed = world
        .alice
        .storage()
        .ux()
        .load_own_card_repropagate()
        .unwrap();
    assert_eq!(owed.failed_attempts, 0);
    assert!(
        world
            .alice
            .is_field_visible_by_label(&world.bob_at_alice, "work")
            .unwrap()
    );
}

// @scenario: multi_device_sync :: Registry presence alone never switches sends off the legacy session
// @internal
#[test]
fn deferred_passes_neither_spend_retries_nor_emit_ciphertext_or_advance_versions() {
    let world = pending_activation();
    wait_for_activation(&world);

    let owed = world
        .alice
        .storage()
        .ux()
        .load_own_card_repropagate()
        .unwrap();
    assert_eq!(owed.failed_attempts, 0);
    assert_eq!(
        world
            .alice
            .storage()
            .pending()
            .get_pending_updates(&world.bob_at_alice)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        world
            .alice
            .storage()
            .contacts()
            .last_sent_delta_version(&world.bob_at_alice)
            .unwrap(),
        0
    );
}

// @scenario: multi_device_sync :: Siblings adopt relayed activation state
// @internal
#[test]
fn sibling_activation_delivers_the_owed_field_after_deferred_passes() {
    let world = pending_activation();
    wait_for_activation(&world);

    let applied = world
        .alice
        .apply_sync_items(vec![activation_item(&world)])
        .unwrap();
    assert_eq!(applied, 1);
    assert_owed_field_delivered(&world);
}

// @scenario: multi_device_sync :: A joining device inherits activation state via full sync
// @internal
#[test]
fn full_sync_activation_delivers_the_owed_field_after_deferred_passes() {
    let world = pending_activation();
    wait_for_activation(&world);

    apply_full_activation(&world).unwrap();

    assert_owed_field_delivered(&world);
}

fn assert_owed_field_delivered(world: &PendingActivation) {
    world.alice.run_owed_repropagation().unwrap();

    let updates = world
        .alice
        .storage()
        .pending()
        .get_pending_updates(&world.bob_at_alice)
        .unwrap();
    assert_eq!(updates.len(), 1);
    for update in updates {
        process_single_card_update_for_device(
            world.bob.identity().unwrap(),
            world.bob.storage(),
            &world.alice_at_bob,
            world.alice.identity().unwrap().device_id(),
            &update.payload,
        )
        .unwrap();
    }
    let contact = world.bob.get_contact(&world.alice_at_bob).unwrap().unwrap();
    let work: Vec<_> = contact
        .card()
        .fields()
        .iter()
        .filter(|field| field.label() == "work")
        .map(|field| field.value())
        .collect();
    assert_eq!(work, vec!["alice@example.com"]);
    let owed = world
        .alice
        .storage()
        .ux()
        .load_own_card_repropagate()
        .unwrap();
    assert_eq!((owed.needs_repropagate, owed.failed_attempts), (false, 0));
}
