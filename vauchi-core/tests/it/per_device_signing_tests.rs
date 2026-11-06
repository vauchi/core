// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Per-device signing of registry broadcasts (ADR-079, SC-62, private#970).
//!
//! These prove attribution and pinning. A device holding the seed can still
//! mint a new device of its own, which is residual D-10 and is pinned here too
//! so that nobody reads these tests as containment.

use crate::common::device_sync::create_test_device;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::{DeviceError, DeviceRegistry, PinnedDeviceKeys, RegistryBroadcast};

const NOW: u64 = 1_000;

fn pins_after(seen: &[&RegistryBroadcast]) -> PinnedDeviceKeys {
    let identity_key = SigningKeyPair::from_seed(&[0x42u8; 32]).public_key();
    let mut pins = PinnedDeviceKeys::default();
    for broadcast in seen {
        pins.accept(&identity_key, broadcast).unwrap();
    }
    pins
}

// A network attacker has no seed. With no presence marker in the signed bytes it
// could flip a keyless entry to an all-zero key, then poison the pin with it.
// @internal
#[test]
fn a_keyless_entry_cannot_be_rewritten_to_name_a_key() {
    let seed = [0x42u8; 32];
    let identity_key = SigningKeyPair::from_seed(&seed);
    let key_a = SigningKeyPair::generate();
    let device_a = create_test_device(&seed, 0, "A");
    let id_a = *device_a.device_id();
    let mut registry = DeviceRegistry::new(
        device_a
            .to_registered(&seed)
            .with_signing_key(public_bytes(&key_a)),
        &identity_key,
    );
    registry
        .add_device(
            create_test_device(&seed, 1, "B").to_registered(&seed),
            &identity_key,
        )
        .unwrap();
    let honest = RegistryBroadcast::signed_by_device(&registry, &identity_key, id_a, &key_a, NOW);
    let mut wire: serde_json::Value = serde_json::from_str(&honest.to_json()).unwrap();
    wire["active_devices"][1]["signing_public_key"] = serde_json::to_value([0u8; 32]).unwrap();
    let rewritten = RegistryBroadcast::from_json(&wire.to_string()).unwrap();

    assert_eq!(
        honest.verify_device_signed(&identity_key.public_key()),
        Ok(())
    );
    assert_eq!(
        rewritten.verify_device_signed(&identity_key.public_key()),
        Err(DeviceError::InvalidRegistrySignature)
    );
}

// @internal
#[test]
fn a_refused_broadcast_pins_nothing() {
    let f = two_devices();
    let stranger_identity = SigningKeyPair::generate();
    let refused =
        RegistryBroadcast::signed_by_device(&f.registry, &stranger_identity, f.id_a, &f.key_a, NOW);
    let mut pins = PinnedDeviceKeys::default();

    assert_eq!(
        pins.accept(&f.identity_key.public_key(), &refused),
        Err(DeviceError::InvalidRegistrySignature)
    );
    assert_eq!(pins, PinnedDeviceKeys::default());
}

struct Fixture {
    seed: [u8; 32],
    identity_key: SigningKeyPair,
    key_a: SigningKeyPair,
    key_b: SigningKeyPair,
    id_a: [u8; 32],
    id_b: [u8; 32],
    registry: DeviceRegistry,
}

fn public_bytes(key: &SigningKeyPair) -> [u8; 32] {
    *key.public_key().as_bytes()
}

fn two_devices() -> Fixture {
    let seed = [0x42u8; 32];
    let identity_key = SigningKeyPair::from_seed(&seed);
    let key_a = SigningKeyPair::generate();
    let key_b = SigningKeyPair::generate();
    let device_a = create_test_device(&seed, 0, "A");
    let device_b = create_test_device(&seed, 1, "B");
    let id_a = *device_a.device_id();
    let id_b = *device_b.device_id();
    let mut registry = DeviceRegistry::new(
        device_a
            .to_registered(&seed)
            .with_signing_key(public_bytes(&key_a)),
        &identity_key,
    );
    registry
        .add_device(
            device_b
                .to_registered(&seed)
                .with_signing_key(public_bytes(&key_b)),
            &identity_key,
        )
        .unwrap();
    Fixture {
        seed,
        identity_key,
        key_a,
        key_b,
        id_a,
        id_b,
        registry,
    }
}

// @internal
#[test]
fn a_device_signed_broadcast_verifies_under_that_devices_key() {
    let f = two_devices();

    let broadcast =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_a, &f.key_a, NOW);

    assert_eq!(
        broadcast.verify_device_signed(&f.identity_key.public_key()),
        Ok(())
    );
    assert_eq!(broadcast.signer_device_id(), Some(f.id_a));
}

// @internal
#[test]
fn a_linked_device_cannot_forge_a_registry_broadcast_for_another_device() {
    let f = two_devices();

    let claiming_b_signed_by_a =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_b, &f.key_a, NOW);

    assert_eq!(
        claiming_b_signed_by_a.verify_device_signed(&f.identity_key.public_key()),
        Err(DeviceError::InvalidDeviceSignature)
    );
}

// A holds the seed, so it can re-sign the registry with B's entry pointing at
// A's own key. The identity signature then holds; only the pin refuses it.
// @internal
#[test]
fn rewriting_another_devices_key_with_the_seed_is_refused_by_the_pin() {
    let f = two_devices();
    let held =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_a, &f.key_a, NOW);
    let mut rewritten_registry = DeviceRegistry::new(
        create_test_device(&f.seed, 0, "A")
            .to_registered(&f.seed)
            .with_signing_key(public_bytes(&f.key_a)),
        &f.identity_key,
    );
    rewritten_registry
        .add_device(
            create_test_device(&f.seed, 1, "B")
                .to_registered(&f.seed)
                .with_signing_key(public_bytes(&f.key_a)),
            &f.identity_key,
        )
        .unwrap();
    let forged = RegistryBroadcast::signed_by_device(
        &rewritten_registry,
        &f.identity_key,
        f.id_b,
        &f.key_a,
        NOW,
    );

    assert_eq!(
        forged.verify_device_signed(&f.identity_key.public_key()),
        Ok(()),
        "without a pin the seed holder's rewrite verifies, which is why the pin exists"
    );
    assert_eq!(
        forged.verify_against_pinned(&f.identity_key.public_key(), &pins_after(&[&held])),
        Err(DeviceError::DeviceKeyChanged)
    );
}

// The seed holder drops B from the registry, then adds B back under its own key:
// a pin that lives only in the last broadcast has forgotten B by then.
// @internal
#[test]
fn removing_a_device_and_adding_it_back_cannot_change_its_pinned_key() {
    let f = two_devices();
    let first =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_a, &f.key_a, NOW);
    let without_b = DeviceRegistry::new(
        create_test_device(&f.seed, 0, "A")
            .to_registered(&f.seed)
            .with_signing_key(public_bytes(&f.key_a)),
        &f.identity_key,
    );
    let held_after_removal =
        RegistryBroadcast::signed_by_device(&without_b, &f.identity_key, f.id_a, &f.key_a, NOW);
    assert_eq!(
        held_after_removal
            .verify_against_pinned(&f.identity_key.public_key(), &pins_after(&[&first])),
        Ok(())
    );
    let mut b_again = DeviceRegistry::new(
        create_test_device(&f.seed, 0, "A")
            .to_registered(&f.seed)
            .with_signing_key(public_bytes(&f.key_a)),
        &f.identity_key,
    );
    b_again
        .add_device(
            create_test_device(&f.seed, 1, "B")
                .to_registered(&f.seed)
                .with_signing_key(public_bytes(&f.key_a)),
            &f.identity_key,
        )
        .unwrap();
    let forged =
        RegistryBroadcast::signed_by_device(&b_again, &f.identity_key, f.id_b, &f.key_a, NOW);

    assert_eq!(
        forged.verify_against_pinned(
            &f.identity_key.public_key(),
            &pins_after(&[&first, &held_after_removal])
        ),
        Err(DeviceError::DeviceKeyChanged)
    );
}

// @internal
#[test]
fn revoking_one_device_invalidates_only_that_devices_signatures() {
    let mut f = two_devices();
    let held =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_a, &f.key_a, NOW);
    f.registry
        .revoke_device(&f.id_b, &f.identity_key, NOW)
        .unwrap();

    let by_revoked =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_b, &f.key_b, NOW);
    let by_remaining =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_a, &f.key_a, NOW);

    assert_eq!(
        by_revoked.verify_against_pinned(&f.identity_key.public_key(), &pins_after(&[&held])),
        Err(DeviceError::SignerNotActive)
    );
    assert_eq!(
        by_remaining.verify_against_pinned(&f.identity_key.public_key(), &pins_after(&[&held])),
        Ok(())
    );
}

// @internal
#[test]
fn a_broadcast_with_no_device_signature_is_refused() {
    let f = two_devices();

    let unsigned = RegistryBroadcast::new(&f.registry, &f.identity_key, NOW);

    assert_eq!(
        unsigned.verify_device_signed(&f.identity_key.public_key()),
        Err(DeviceError::InvalidDeviceSignature)
    );
}

// @internal
#[test]
fn a_device_signature_from_a_different_identity_is_refused() {
    let f = two_devices();
    let stranger_identity = SigningKeyPair::generate();

    let broadcast =
        RegistryBroadcast::signed_by_device(&f.registry, &stranger_identity, f.id_a, &f.key_a, NOW);

    assert_eq!(
        broadcast.verify_device_signed(&f.identity_key.public_key()),
        Err(DeviceError::InvalidRegistrySignature)
    );
}

// Residual D-10: not containment. A seed holder can add a new device of its own,
// and a contact accepts it. This test documents the limit; #1098 changes it.
// @internal
#[test]
fn a_seed_holder_can_still_mint_a_new_device_of_its_own() {
    let mut f = two_devices();
    let held =
        RegistryBroadcast::signed_by_device(&f.registry, &f.identity_key, f.id_a, &f.key_a, NOW);
    let attacker_key = SigningKeyPair::generate();
    let attacker_device = create_test_device(&f.seed, 2, "attacker");
    let attacker_id = *attacker_device.device_id();
    f.registry
        .add_device(
            attacker_device
                .to_registered(&f.seed)
                .with_signing_key(public_bytes(&attacker_key)),
            &f.identity_key,
        )
        .unwrap();

    let by_attacker = RegistryBroadcast::signed_by_device(
        &f.registry,
        &f.identity_key,
        attacker_id,
        &attacker_key,
        NOW,
    );

    assert_eq!(
        by_attacker.verify_against_pinned(&f.identity_key.public_key(), &pins_after(&[&held])),
        Ok(())
    );
}
