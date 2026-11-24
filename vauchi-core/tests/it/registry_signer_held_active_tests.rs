// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact refuses a registry signed by a device it pinned that its held
//! registry no longer lists. Every linked device holds the identity key, so
//! without this a revoked device signs a registry that un-revokes itself
//! (vauchi/private#1391). A device never seen, such as a restored install,
//! may still sign.

use crate::common::per_device::{Party, refused_with, tablet_entry, two_parties};
use vauchi_core::api::ReceiveOutcome;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::{DeviceRegistry, RegistryBroadcast};

const TABLET: [u8; 32] = [7u8; 32];

struct Revoked {
    p: Party,
    tablet_key: SigningKeyPair,
    revoking: DeviceRegistry,
}

fn bob_revokes_his_tablet() -> Revoked {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let (phone_id, phone_key, now) = (p.bob_device_id(), p.bob_device_key(), p.now());
    let tablet_key = SigningKeyPair::generate();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), identity_key)
        .unwrap();
    let with_tablet =
        RegistryBroadcast::signed_by_device(&registry, identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&with_tablet).unwrap();
    registry.revoke_device(&TABLET, identity_key, now).unwrap();
    let without_tablet =
        RegistryBroadcast::signed_by_device(&registry, identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&without_tablet).unwrap();
    Revoked {
        p,
        tablet_key,
        revoking: registry,
    }
}

/// What a copy of the tablet can build without the registry API: the
/// revoking registry with its own entry un-revoked and the version raised.
fn un_revoked_by_hand(registry: &DeviceRegistry) -> DeviceRegistry {
    let mut value = serde_json::to_value(registry).unwrap();
    let tablet_id = serde_json::to_value(TABLET).unwrap();
    for device in value["devices"].as_array_mut().unwrap() {
        if device["device_id"] == tablet_id {
            device["revoked"] = false.into();
            device["revoked_at"] = serde_json::Value::Null;
        }
    }
    let version = value["version"].as_u64().unwrap();
    value["version"] = (version + 1).into();
    serde_json::from_value(value).unwrap()
}

fn held_version(p: &Party) -> u64 {
    p.alice
        .storage()
        .device()
        .load_contact_device_registry(&p.bob_at_alice)
        .unwrap()
        .unwrap()
        .version()
}

// @scenario: device_management :: Lost device revocation
// @internal
#[test]
fn a_revoked_device_cannot_sign_a_registry_that_un_revokes_it() {
    let r = bob_revokes_his_tablet();
    let identity_key = r.p.bob.identity().unwrap().signing_keypair();
    let revoking_version = r.revoking.version();
    let forged = RegistryBroadcast::signed_by_device(
        &un_revoked_by_hand(&r.revoking),
        identity_key,
        TABLET,
        &r.tablet_key,
        r.p.now(),
    );

    refused_with(
        r.p.push_to_alice(&forged),
        "was removed from the held registry",
    );
    assert_eq!(held_version(&r.p), revoking_version);
    let active =
        r.p.alice
            .storage()
            .device()
            .load_contact_active_devices(&r.p.bob_at_alice)
            .unwrap();
    assert!(
        active.iter().all(|device| device.device_id != TABLET),
        "the tablet stays revoked"
    );
}

// @scenario: device_management :: Lost device revocation
// @internal
#[test]
fn a_device_still_active_signs_the_next_registry() {
    let r = bob_revokes_his_tablet();
    let identity_key = r.p.bob.identity().unwrap().signing_keypair();
    let mut next = r.revoking.clone();
    let laptop_key = SigningKeyPair::generate();
    let mut laptop = tablet_entry(&laptop_key);
    laptop.device_id = [9u8; 32];
    next.add_device(laptop, identity_key).unwrap();
    let signed_by_phone = RegistryBroadcast::signed_by_device(
        &next,
        identity_key,
        r.p.bob_device_id(),
        &r.p.bob_device_key(),
        r.p.now(),
    );

    let outcome = r.p.push_to_alice(&signed_by_phone).unwrap();

    assert!(matches!(outcome, ReceiveOutcome::RegistryPushReceived(_)));
    assert_eq!(held_version(&r.p), next.version());
}
