// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact's pinned device keys travel with its registry between this
//! identity's own devices, by owner sync and by device linking, and a
//! device keeps its own pin when a carried one differs (private#970,
//! owner 2026-11-23). Otherwise a seed holder removes a device and adds it
//! back under its own key: the device that saw it first refuses, a sibling
//! that learned the contact later accepts.

use crate::common::device_sync::{
    create_test_device, create_test_storage, device_signed_broadcast, journal_for_tablet,
    link_tablet,
};
use crate::common::per_device::{Party, refused_with, tablet_entry, two_parties};
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::{PinnedDeviceKeys, RegistryBroadcast};
use vauchi_core::sync::{DeviceLinkIntent, SyncItem};

/// The id `tablet_entry` gives Bob's departed tablet.
const DEPARTED: [u8; 32] = [7u8; 32];

fn key_of(pair: &SigningKeyPair) -> [u8; 32] {
    *pair.public_key().as_bytes()
}

/// The item one of Alice's devices journals for Bob's registry.
fn relayed(p: &Party, broadcast: &RegistryBroadcast, pins: PinnedDeviceKeys) -> SyncItem {
    SyncItem::ContactRegistryReceived {
        contact_id: p.bob_at_alice.clone(),
        registry_json: broadcast.to_json(),
        version: broadcast.version(),
        timestamp: p.now(),
        pins,
    }
}

/// Bob's pins as Alice's other device holds them: his phone, and a tablet
/// that has since left his registry.
fn pins_with_a_departed_tablet(p: &Party, tablet_key: &SigningKeyPair) -> PinnedDeviceKeys {
    PinnedDeviceKeys::from_pairs([
        (p.bob_device_id(), key_of(&p.bob_device_key())),
        (DEPARTED, key_of(tablet_key)),
    ])
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_sibling_adopts_a_relayed_pin_for_a_device_the_broadcast_no_longer_names() {
    let p = two_parties();
    let honest = device_signed_broadcast(&p.bob);
    let tablet_key = SigningKeyPair::generate();

    p.alice
        .apply_sync_items(vec![relayed(
            &p,
            &honest,
            pins_with_a_departed_tablet(&p, &tablet_key),
        )])
        .unwrap();

    assert_eq!(p.alice_pin_for(&DEPARTED), Some(key_of(&tablet_key)));
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_relayed_pin_refuses_a_device_re_added_under_a_new_key() {
    let p = two_parties();
    let honest = device_signed_broadcast(&p.bob);
    let tablet_key = SigningKeyPair::generate();
    p.alice
        .apply_sync_items(vec![relayed(
            &p,
            &honest,
            pins_with_a_departed_tablet(&p, &tablet_key),
        )])
        .unwrap();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let mut re_added = p.bob_registry();
    re_added
        .add_device(tablet_entry(&SigningKeyPair::generate()), identity_key)
        .unwrap();
    re_added.lift_version_to(honest.version() + 1, identity_key);
    let forged = RegistryBroadcast::signed_by_device(
        &re_added,
        identity_key,
        p.bob_device_id(),
        &p.bob_device_key(),
        p.now(),
    );

    refused_with(p.push_to_alice(&forged), "signing key changed");
    assert_eq!(p.alice_pin_for(&DEPARTED), Some(key_of(&tablet_key)));
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_sibling_keeps_its_own_pin_when_a_relayed_one_differs() {
    let p = two_parties();
    let honest = device_signed_broadcast(&p.bob);
    p.push_to_alice(&honest).unwrap();
    let held = p.alice_pin_for(&p.bob_device_id());
    let other =
        PinnedDeviceKeys::from_pairs([(p.bob_device_id(), key_of(&SigningKeyPair::generate()))]);

    p.alice
        .apply_sync_items(vec![relayed(&p, &honest, other)])
        .unwrap();

    assert_eq!(p.alice_pin_for(&p.bob_device_id()), held);
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn the_registry_journaled_for_a_sibling_carries_the_contacts_pins() {
    let p = two_parties();
    let (registry, tablet) = link_tablet(&p.alice);

    p.push_to_alice(&device_signed_broadcast(&p.bob)).unwrap();

    let journal = journal_for_tablet(&p.alice, registry, &tablet);
    let pins = journal
        .iter()
        .find_map(|item| match item {
            SyncItem::ContactRegistryReceived { pins, .. } => Some(pins.clone()),
            _ => None,
        })
        .expect("the push journals Bob's registry for the tablet");
    assert_eq!(
        pins.pinned(&p.bob_device_id()),
        Some(key_of(&p.bob_device_key()))
    );
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_linked_device_inherits_a_pin_for_a_device_the_broadcast_no_longer_names() {
    let p = two_parties();
    p.push_to_alice(&device_signed_broadcast(&p.bob)).unwrap();
    let tablet_key = SigningKeyPair::generate();
    p.alice
        .storage()
        .device()
        .save_contact_device_pins(
            &p.bob_at_alice,
            &pins_with_a_departed_tablet(&p, &tablet_key),
        )
        .unwrap();
    let alice = p.alice.identity().unwrap();
    let seed = *alice.master_seed();
    let payload = DeviceSyncOrchestrator::new(
        p.alice.storage(),
        alice.create_device_info(0),
        alice.initial_device_registry(),
    )
    .create_full_sync_payload(DeviceLinkIntent::AddDevice)
    .unwrap();

    let laptop = create_test_storage();
    let laptop_device = create_test_device(&seed, 1, "laptop");
    let laptop_registry = alice.initial_device_registry();
    DeviceSyncOrchestrator::new(&laptop, laptop_device, laptop_registry)
        .apply_full_sync(payload)
        .unwrap();

    let inherited = laptop
        .device()
        .load_contact_device_pins(&p.bob_at_alice)
        .unwrap()
        .expect("the link carries Bob's registry");
    assert_eq!(inherited.pinned(&DEPARTED), Some(key_of(&tablet_key)));
}
