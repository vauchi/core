// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Losing one of two devices needs no recovery: the other device revokes
//! it, and contacts learn of the revocation through the registry push.

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::linked_peer::link_contacts;
use vauchi_core::Vauchi;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::identity::DeviceInfo;

/// Delivers every update `from` has queued for `to`, removing each from
/// `from`'s queue as a relay hand-off would, and returns what `to` made of
/// each.
fn deliver(
    from: &Vauchi,
    to_id_in_from: &str,
    to: &Vauchi,
    from_id_in_to: &str,
) -> Vec<ReceiveOutcome> {
    let queued = from
        .storage()
        .pending()
        .get_pending_updates(to_id_in_from)
        .unwrap();
    queued
        .into_iter()
        .map(|update| {
            from.storage()
                .pending()
                .delete_pending_update(&update.id)
                .unwrap();
            process_single_card_update(
                to.identity().unwrap(),
                to.storage(),
                from_id_in_to,
                &update.payload,
            )
            .unwrap()
        })
        .collect()
}

// @scenario: contact_recovery :: Recovery when user has multiple devices
#[test]
fn revoking_a_lost_laptop_from_the_phone_reaches_contacts() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let linked = link_contacts(&bob, &alice, "Alice");
    let (bob_in_alice, alice_in_bob) = (&linked.host_contact_id, &linked.peer_contact_id);
    let seed = [9u8; 32];
    let laptop = DeviceInfo::derive(&seed, 1, "Laptop".into(), 0).to_registered(&seed);
    let laptop_id = laptop.device_id;
    let mut registry = alice.identity().unwrap().initial_device_registry();
    registry.add_device_unsigned(laptop).unwrap();
    alice
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    let alices_registry_at_bob = || {
        bob.storage()
            .device()
            .load_contact_device_registry(alice_in_bob)
            .unwrap()
            .unwrap()
    };

    alice.queue_registry_pushes().unwrap();
    for outcome in deliver(&alice, bob_in_alice, &bob, alice_in_bob) {
        if let ReceiveOutcome::RegistryPushReceived(reply) = outcome {
            bob.queue_registry_ack(&reply).unwrap();
        }
    }
    deliver(&bob, alice_in_bob, &alice, bob_in_alice);
    assert!(alices_registry_at_bob().contains_device(&laptop_id));

    alice.revoke_device(1).unwrap();
    assert_eq!(alice.queue_registry_pushes().unwrap(), 1);
    deliver(&alice, bob_in_alice, &bob, alice_in_bob);

    let at_bob = alices_registry_at_bob();
    assert!(!at_bob.contains_device(&laptop_id));
    assert_eq!(at_bob.active_device_count(), 1);
}
