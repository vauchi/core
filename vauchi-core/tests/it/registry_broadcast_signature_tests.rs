// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device registry and its broadcast: the broadcast's signature covers its
//! contents and its freshness windows are exact, and the registry's
//! version, device index and primary transfer behave as documented
//! (vauchi/private#522).

use vauchi_core::Identity;
use vauchi_core::identity::RegistryBroadcast;

fn tampered(broadcast: &RegistryBroadcast, field: &str, value: u64) -> RegistryBroadcast {
    let mut json = serde_json::to_value(broadcast).unwrap();
    json[field] = value.into();
    serde_json::from_value(json).unwrap()
}

// @internal
#[test]
fn a_broadcast_changed_after_signing_does_not_verify() {
    let alice = Identity::create("Alice", 0);
    let registry = alice.initial_device_registry();
    let broadcast = RegistryBroadcast::new(&registry, alice.signing_keypair(), 1_000);
    let key = &vauchi_core::crypto::PublicKey::from_bytes(*alice.signing_public_key());

    assert!(broadcast.verify(key));
    assert!(!tampered(&broadcast, "timestamp", 2_000).verify(key));
    assert!(!tampered(&broadcast, "version", broadcast.version() + 1).verify(key));
}

// A broadcast is fresh from `max_age` seconds old up to 60 seconds in the
// future (clock skew), and stale one second beyond either edge.
// @internal
#[test]
fn a_broadcast_is_fresh_within_its_age_and_skew_windows() {
    let alice = Identity::create("Alice", 0);
    let key = vauchi_core::crypto::PublicKey::from_bytes(*alice.signing_public_key());
    let stamped = 10_000;
    let broadcast = RegistryBroadcast::new(
        &alice.initial_device_registry(),
        alice.signing_keypair(),
        stamped,
    );
    let fresh_at = |now| broadcast.verify_with_freshness(&key, 0, now, 300).is_ok();

    assert_eq!(broadcast.timestamp(), stamped);
    assert!(fresh_at(stamped));
    assert!(fresh_at(stamped + 300));
    assert!(!fresh_at(stamped + 301));
    assert!(fresh_at(stamped - 60));
    assert!(!fresh_at(stamped - 61));
}

fn registry_of_three() -> (
    Identity,
    vauchi_core::identity::DeviceRegistry,
    Vec<[u8; 32]>,
) {
    use vauchi_core::identity::DeviceInfo;

    let alice = Identity::create("Alice", 0);
    let mut registry = alice.initial_device_registry();
    let mut ids = vec![registry.all_devices()[0].device_id];
    for index in 1..3 {
        let seed = [index as u8; 32];
        let device =
            DeviceInfo::derive(&seed, index, format!("Device {index}"), 0).to_registered(&seed);
        ids.push(device.device_id);
        registry.add_device_unsigned(device).unwrap();
    }
    (alice, registry, ids)
}

// @internal
#[test]
fn each_added_device_bumps_the_registry_version_and_next_index() {
    let alice = Identity::create("Alice", 0);
    let fresh = alice.initial_device_registry();
    let (_, registry, _) = registry_of_three();

    assert_eq!(registry.version(), fresh.version() + 2);
    assert_eq!(registry.next_device_index(), 3);
}

// @internal
#[test]
fn transferring_primary_moves_the_device_first_and_re_signs() {
    use vauchi_core::identity::DeviceError;

    let (alice, mut registry, ids) = registry_of_three();
    let key = vauchi_core::crypto::PublicKey::from_bytes(*alice.signing_public_key());
    let before = registry.version();

    registry
        .transfer_primary(&ids[0], alice.signing_keypair())
        .unwrap();
    assert_eq!(
        registry.version(),
        before,
        "already primary: nothing changes"
    );

    registry
        .transfer_primary(&ids[2], alice.signing_keypair())
        .unwrap();
    assert_eq!(registry.primary_device().unwrap().device_id, ids[2]);
    assert_eq!(registry.version(), before + 1);
    assert!(registry.verify(&key));

    assert!(matches!(
        registry.transfer_primary(&[0xEE; 32], alice.signing_keypair()),
        Err(DeviceError::DeviceNotFound)
    ));
}

// @internal
#[test]
fn identical_public_keys_are_an_identity_collision() {
    use vauchi_core::identity::check_identity_collision;

    assert!(check_identity_collision(&[1u8; 32], &[1u8; 32]));
    assert!(!check_identity_collision(&[1u8; 32], &[2u8; 32]));
}

// @internal
#[test]
fn a_device_keeps_the_time_it_was_created() {
    let device = vauchi_core::identity::DeviceInfo::derive(&[3u8; 32], 1, "Laptop".into(), 4_242);

    assert_eq!(device.created_at(), 4_242);
}
