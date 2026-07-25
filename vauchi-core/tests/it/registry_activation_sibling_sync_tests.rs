// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 sibling relay — reader half (readers-before-writers).
//!
//! Linked devices receive the identity's contact registries and activation
//! handshake state via owner sync, so any sibling can resolve the contact's
//! device-scoped tokens and continue a handshake the exchanging device
//! started (mailbox tokens are identity-scoped — an ack may be fetched by a
//! different device than the one that pushed). The tolerant SyncItem reader
//! (shipped 2026-07-21) makes these new variants mixed-version safe.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::identity::RegistryBroadcast;
use vauchi_core::sync::SyncItem;
use vauchi_core::sync::registry_activation::{ActivationState, ActivationTracker};

/// A device holding one exchanged contact (Bob), as a sibling that never
/// exchanged with him would after `ContactAdded` sync.
fn device_with_contact() -> (vauchi_core::Vauchi, vauchi_core::Vauchi, String) {
    let device = create_vauchi_with_identity("Alice");
    let bob_wb = create_vauchi_with_identity("Bob");
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), SymmetricKey::generate(), 0);
    let contact_id = contact.id().to_string();
    device.add_contact(contact).unwrap();
    (device, bob_wb, contact_id)
}

fn bob_broadcast(bob_wb: &vauchi_core::Vauchi) -> RegistryBroadcast {
    let bob_identity = bob_wb.identity().unwrap();
    RegistryBroadcast::new(
        &bob_identity.initial_device_registry(),
        bob_identity.signing_keypair(),
        1_753_000_000,
    )
}

// @scenario: multi_device_sync :: Siblings learn a contact's registry through owner sync
// @internal
#[test]
fn contact_registry_received_item_persists_the_verified_registry() {
    let (device, bob_wb, contact_id) = device_with_contact();
    let broadcast = bob_broadcast(&bob_wb);

    let applied = device
        .apply_sync_items(vec![SyncItem::ContactRegistryReceived {
            contact_id: contact_id.clone(),
            registry_json: broadcast.to_json(),
            version: broadcast.version(),
            timestamp: 1_753_000_001,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    assert!(
        !device
            .storage()
            .device()
            .load_contact_active_devices(&contact_id)
            .unwrap()
            .is_empty(),
        "sibling can now resolve the contact's device-scoped tokens"
    );
}

// @scenario: multi_device_sync :: A forged relayed registry is rejected
// @internal
#[test]
fn contact_registry_received_item_rejects_a_wrong_signer() {
    let (device, _bob_wb, contact_id) = device_with_contact();
    let mallory = create_vauchi_with_identity("Mallory");
    let forged = bob_broadcast(&mallory);

    let applied = device
        .apply_sync_items(vec![SyncItem::ContactRegistryReceived {
            contact_id: contact_id.clone(),
            registry_json: forged.to_json(),
            version: forged.version(),
            timestamp: 1_753_000_001,
        }])
        .unwrap();

    // The tolerant apply skips bad items rather than failing the batch —
    // the invariant is that a forged registry is never counted or persisted.
    assert_eq!(applied, 0, "forged relayed registry must not apply");
    assert!(
        device
            .storage()
            .device()
            .load_contact_active_devices(&contact_id)
            .unwrap()
            .is_empty()
    );
}

// @scenario: multi_device_sync :: Siblings adopt relayed activation state
// @internal
#[test]
fn contact_activation_changed_item_seeds_a_fresh_tracker() {
    let (device, _bob_wb, contact_id) = device_with_contact();

    let applied = device
        .apply_sync_items(vec![SyncItem::ContactActivationChanged {
            contact_id: contact_id.clone(),
            push_nonce: Some(vec![7u8; 32]),
            pushed_version: Some(2),
            our_version_acked: Some(2),
            peer_version_held: Some(5),
            timestamp: 1_753_000_002,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    let tracker = device
        .storage()
        .registry_activation()
        .load_activation(&contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.state(), ActivationState::Active);
    assert_eq!(tracker.our_version_acked(), Some(2));
    assert_eq!(tracker.peer_version_held(), Some(5));
    let (nonce, version) = tracker.outstanding_push().expect("outstanding push");
    assert_eq!((nonce, version), ([7u8; 32], 2));
}

// @scenario: multi_device_sync :: Relayed activation merges monotonically, never regressing
// @internal
#[test]
fn contact_activation_changed_item_merges_without_regressing_local_state() {
    let (device, _bob_wb, contact_id) = device_with_contact();

    // Local device already progressed to a NEWER push than the relayed one.
    let mut local = ActivationTracker::new();
    local.record_push_sent([9u8; 32], 4);
    local.record_peer_registry(6);
    device
        .storage()
        .registry_activation()
        .save_activation(&contact_id, &local)
        .unwrap();

    device
        .apply_sync_items(vec![SyncItem::ContactActivationChanged {
            contact_id: contact_id.clone(),
            push_nonce: Some(vec![7u8; 32]),
            pushed_version: Some(2),
            our_version_acked: Some(2),
            peer_version_held: Some(5),
            timestamp: 1_753_000_003,
        }])
        .unwrap();

    let tracker = device
        .storage()
        .registry_activation()
        .load_activation(&contact_id)
        .unwrap()
        .expect("tracker row");
    let (nonce, version) = tracker.outstanding_push().expect("outstanding push");
    assert_eq!(
        (nonce, version),
        ([9u8; 32], 4),
        "a newer local push is never replaced by an older relayed one"
    );
    assert_eq!(
        tracker.peer_version_held(),
        Some(6),
        "held peer version merges as max"
    );
    assert_eq!(
        tracker.our_version_acked(),
        Some(2),
        "acked version merges as max"
    );
}
