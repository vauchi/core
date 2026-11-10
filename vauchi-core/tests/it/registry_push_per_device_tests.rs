// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact accepts a registry broadcast only when the device it names
//! as signer is active and signed it with the key the contact pinned for
//! it (vauchi/private#970, SC-62). Everything goes through the public
//! receive path a relay delivery takes.

use crate::common::device_sync::device_signed_broadcast;
use crate::common::per_device::{refused_with, tablet_entry, two_parties};
use vauchi_core::api::ReceiveOutcome;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::RegistryBroadcast;

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_device_signed_push_is_accepted_and_its_key_pinned() {
    let p = two_parties();
    let honest = device_signed_broadcast(&p.bob);

    let outcome = p.push_to_alice(&honest).unwrap();

    assert!(matches!(outcome, ReceiveOutcome::RegistryPushReceived(_)));
    assert_eq!(
        p.alice_pin_for(&p.bob_device_id()),
        Some(*p.bob_device_key().public_key().as_bytes()),
        "Alice pinned Bob's phone key on first sight"
    );
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_broadcast_signed_by_the_identity_alone_is_refused() {
    let p = two_parties();
    let identity_only = RegistryBroadcast::new(
        &p.bob_registry(),
        p.bob.identity().unwrap().signing_keypair(),
        p.now(),
    );

    let result = p.push_to_alice(&identity_only);

    refused_with(result, "Invalid device signature");
    assert_eq!(
        p.alice_pin_for(&p.bob_device_id()),
        None,
        "a refused broadcast pins nothing"
    );
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_linked_device_cannot_sign_a_broadcast_as_another_device() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let tablet_key = SigningKeyPair::generate();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), identity_key)
        .unwrap();
    let phone_claims_tablet = RegistryBroadcast::signed_by_device(
        &registry,
        identity_key,
        [7u8; 32],
        &p.bob_device_key(),
        p.now(),
    );

    let result = p.push_to_alice(&phone_claims_tablet);

    refused_with(result, "Invalid device signature");
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_signer_that_is_not_an_active_device_is_refused() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let stranger = SigningKeyPair::generate();
    let as_unknown_device = RegistryBroadcast::signed_by_device(
        &p.bob_registry(),
        identity_key,
        [7u8; 32],
        &stranger,
        p.now(),
    );

    let result = p.push_to_alice(&as_unknown_device);

    refused_with(result, "not an active device");
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_known_devices_key_cannot_change_even_with_the_identity_key() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let honest = device_signed_broadcast(&p.bob);
    p.push_to_alice(&honest).unwrap();
    let rekey = SigningKeyPair::generate();
    let mut rewritten = p.bob.identity().unwrap().initial_device_registry();
    rewritten
        .name_device_key(
            &p.bob_device_id(),
            *rekey.public_key().as_bytes(),
            identity_key,
        )
        .unwrap();
    assert!(
        rewritten.version() > p.bob_registry().version(),
        "newer than the pinned one"
    );
    let rekeyed = RegistryBroadcast::signed_by_device(
        &rewritten,
        identity_key,
        p.bob_device_id(),
        &rekey,
        p.now(),
    );

    let result = p.push_to_alice(&rekeyed);

    refused_with(result, "signing key changed");
    assert_eq!(
        p.alice_pin_for(&p.bob_device_id()),
        Some(*p.bob_device_key().public_key().as_bytes()),
        "the first key stays pinned"
    );
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_removed_device_cannot_sign_and_its_pin_is_kept() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let tablet_key = SigningKeyPair::generate();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), identity_key)
        .unwrap();
    let with_tablet = RegistryBroadcast::signed_by_device(
        &registry,
        identity_key,
        [7u8; 32],
        &tablet_key,
        p.now(),
    );
    p.push_to_alice(&with_tablet).unwrap();
    assert_eq!(
        p.alice_pin_for(&[7u8; 32]),
        Some(*tablet_key.public_key().as_bytes())
    );
    registry
        .revoke_device(&[7u8; 32], identity_key, p.now())
        .unwrap();

    let removed_signs = RegistryBroadcast::signed_by_device(
        &registry,
        identity_key,
        [7u8; 32],
        &tablet_key,
        p.now(),
    );
    let result = p.push_to_alice(&removed_signs);
    refused_with(result, "not an active device");

    let (phone_id, phone_key, now) = (p.bob_device_id(), p.bob_device_key(), p.now());
    let phone_signs =
        RegistryBroadcast::signed_by_device(&registry, identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&phone_signs).unwrap();
    assert_eq!(
        p.alice_pin_for(&[7u8; 32]),
        Some(*tablet_key.public_key().as_bytes()),
        "a removed device's key stays pinned so it cannot come back under that id"
    );
}
