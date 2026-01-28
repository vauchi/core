// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for exchange::ble
//! Extracted from ble.rs

use std::time::Duration;
use vauchi_core::exchange::*;

#[test]
fn test_ble_discover_nearby_devices() {
    let devices = vec![
        BLEDevice::with_name("device-1", "Alice's Phone", -45),
        BLEDevice::with_name("device-2", "Bob's Phone", -60),
    ];
    let verifier = MockBLEVerifier::new(devices.clone(), 1.5);

    let discovered = verifier.discover_nearby(Duration::from_secs(5)).unwrap();

    assert_eq!(discovered.len(), 2);
    assert_eq!(discovered[0].name.as_deref(), Some("Alice's Phone"));
    assert_eq!(discovered[1].name.as_deref(), Some("Bob's Phone"));
}

#[test]
fn test_ble_accept_within_2_meters() {
    let verifier = MockBLEVerifier::success_at_distance(1.5); // 1.5 meters away
    let device = &verifier.devices[0];

    assert!(verifier.is_within_range(device, 2.0));
    assert!(verifier.verify_device_proximity(device).is_ok());
}

#[test]
fn test_ble_reject_if_too_far() {
    let verifier = MockBLEVerifier::success_at_distance(5.0); // 5 meters away
    let device = &verifier.devices[0];

    assert!(!verifier.is_within_range(device, 2.0));

    let result = verifier.verify_device_proximity(device);
    assert!(matches!(result, Err(ProximityError::TooFar)));
}

#[test]
fn test_ble_device_with_exchange_token() {
    let token = [42u8; 32];
    let device = BLEDevice::new("test-id", -50).with_exchange_token(token);

    assert_eq!(device.exchange_token, Some(token));
}

#[test]
fn test_ble_verifier_failure() {
    let verifier = MockBLEVerifier::failure();

    let result = verifier.discover_nearby(Duration::from_secs(1));
    assert!(result.is_err());
}
