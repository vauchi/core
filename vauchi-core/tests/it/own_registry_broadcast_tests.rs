// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The broadcast a device sends to contacts is signed by that device, and
//! the saved registry it is built from names the device's own key
//! (vauchi/private#970, SC-62).

use crate::common::device_sync::device_signed_broadcast;
use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::Vauchi;
use vauchi_core::identity::Identity;

fn stored_device_key(wb: &Vauchi) -> [u8; 32] {
    *wb.storage()
        .device()
        .load_or_create_device_signing_key(wb.identity().unwrap())
        .unwrap()
        .public_key()
        .as_bytes()
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn this_device_signs_its_own_registry_broadcast() {
    let wb = create_vauchi_with_identity("Alice");
    let identity = wb.identity().unwrap();

    let broadcast = device_signed_broadcast(&wb);

    assert_eq!(broadcast.signer_device_id(), Some(*identity.device_id()));
    assert_eq!(
        broadcast.verify_device_signed(&identity.signing_keypair().public_key()),
        Ok(())
    );
    let signer = broadcast
        .active_devices()
        .iter()
        .find(|device| device.device_id == *identity.device_id())
        .expect("the signer is an active device");
    assert_eq!(signer.signing_public_key, Some(stored_device_key(&wb)));
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_saved_registry_without_this_devices_key_gains_it_once() {
    let wb = create_vauchi_with_identity("Alice");
    let identity = wb.identity().unwrap();
    let identity_pk = identity.signing_keypair().public_key();
    let keyless = identity.initial_device_registry();
    wb.storage()
        .device()
        .save_device_registry(&keyless)
        .unwrap();

    let broadcast = device_signed_broadcast(&wb);

    assert_eq!(broadcast.verify_device_signed(&identity_pk), Ok(()));
    let saved = wb
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    assert_eq!(
        saved
            .find_device(identity.device_id())
            .unwrap()
            .signing_public_key,
        Some(stored_device_key(&wb)),
        "the saved registry now names this device's key"
    );
    assert_eq!(saved.version(), keyless.version() + 1);
    assert!(saved.verify(&identity_pk), "re-signed by the identity");
    let again = device_signed_broadcast(&wb);
    assert_eq!(
        again.version(),
        broadcast.version(),
        "named once, not on every call"
    );
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_saved_registry_naming_another_key_for_this_device_is_not_signed() {
    let wb = create_vauchi_with_identity("Alice");
    let identity = wb.identity().unwrap();
    let foreign = identity.initial_device_registry_naming([9u8; 32]);
    wb.storage()
        .device()
        .save_device_registry(&foreign)
        .unwrap();

    let result = wb.own_registry_broadcast(identity);

    let error =
        result.expect_err("a registry naming another key for this device is not signed as it");
    assert!(
        error.to_string().contains("another signing key"),
        "the refusal names the cause, got: {error}"
    );
}

const ANOTHER_IDENTITYS_REGISTRY: &str = "the saved device registry belongs to another identity";

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_registry_saved_by_a_previous_identity_is_not_signed() {
    let wb = create_vauchi_with_identity("Alice");
    let previous = Identity::create("Bob", 1_700_000_000);
    let left_over = previous.initial_device_registry_naming([9u8; 32]);
    wb.storage()
        .device()
        .save_device_registry(&left_over)
        .unwrap();

    let error = wb
        .own_registry_broadcast(wb.identity().unwrap())
        .expect_err("a registry another identity signed is not this identity's");

    assert!(
        error.to_string().contains(ANOTHER_IDENTITYS_REGISTRY),
        "refused as another identity's registry, got: {error}"
    );
    let still_saved = wb
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    assert_eq!(still_saved.to_json(), left_over.to_json(), "left as it was");
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_registry_naming_this_device_under_another_identitys_signature_is_not_signed() {
    let wb = create_vauchi_with_identity("Alice");
    let identity = wb.identity().unwrap();
    let mut resigned = identity.initial_device_registry_naming(stored_device_key(&wb));
    let other = Identity::create("Bob", 1_700_000_000);
    resigned.lift_version_to(resigned.version() + 1, other.signing_keypair());
    wb.storage()
        .device()
        .save_device_registry(&resigned)
        .unwrap();

    let error = wb
        .own_registry_broadcast(identity)
        .expect_err("this identity does not broadcast a registry it did not sign");

    assert!(
        error.to_string().contains(ANOTHER_IDENTITYS_REGISTRY),
        "refused as another identity's registry, got: {error}"
    );
}
