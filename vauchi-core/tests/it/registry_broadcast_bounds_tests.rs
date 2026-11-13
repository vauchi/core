// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Registry parsing stays bounded at the supported fleet and key widths.

use serde_json::{Value, json};
use vauchi_core::identity::{DeviceError, Identity, MAX_DEVICES, RegistryBroadcast};
use vauchi_core::sync::delta::DeltaError;
use vauchi_core::sync::registry_activation::RegistryPushPayload;

fn broadcast() -> Value {
    let identity = Identity::create("Alice", 0);
    serde_json::to_value(RegistryBroadcast::new(
        &identity.initial_device_registry(),
        identity.signing_keypair(),
        0,
    ))
    .unwrap()
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn registry_parser_rejects_more_than_the_supported_device_count() {
    let mut value = broadcast();
    let entry = value["active_devices"][0].clone();
    value["active_devices"] = Value::Array(vec![entry; MAX_DEVICES + 1]);
    let error = RegistryBroadcast::from_json(&value.to_string())
        .expect_err("over-capacity registry must fail at parsing");
    assert!(matches!(error, DeviceError::MaxDevicesReached), "{error:?}");
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn registry_parser_rejects_noncanonical_device_signature_lengths() {
    for length in [0, 1, 63, 65, 1024] {
        let mut value = broadcast();
        value["signer_device_id"] = json!(vec![1u8; 32]);
        value["device_signature"] = json!(vec![1u8; length]);
        let error = RegistryBroadcast::from_json(&value.to_string())
            .expect_err("invalid device signature length must fail at parsing");
        assert!(
            matches!(error, DeviceError::InvalidDeviceSignature),
            "length {length}: {error:?}"
        );
    }
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn carried_registry_accepts_bounded_raw_json_and_rejects_one_byte_more() {
    let mut encoded = broadcast().to_string().into_bytes();
    encoded.resize(8192, b' ');
    let payload = RegistryPushPayload::new([7; 32], encoded.clone())
        .expect("the bounded raw ceiling must include a supported full registry");
    assert_eq!(payload.broadcast_json(), encoded.as_slice());
    encoded.push(b' ');
    let error = RegistryPushPayload::new([7; 32], encoded.clone())
        .expect_err("raw ceiling plus one byte must fail");
    assert!(matches!(error, DeltaError::InvalidPayload(_)), "{error:?}");
    let error = RegistryBroadcast::from_json(std::str::from_utf8(&encoded).unwrap())
        .expect_err("registry parser must enforce its own raw ceiling");
    assert!(
        matches!(error, DeviceError::InvalidRegistrySignature),
        "{error:?}"
    );
}
