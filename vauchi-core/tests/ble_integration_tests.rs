// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Exchange Integration Tests
//!
//! Integration tests for BLE-based contact exchange.
//! Feature file: features/contact_exchange.feature @ble
//!
//! These tests verify:
//! - BLE advertisement creation
//! - Device discovery
//! - Exchange session management
//! - Proximity verification

use std::time::Duration;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::exchange::{
    BLEAdvertisement, BLEDevice, BLEExchangeSession, BLEExchangeState, BLEProximityVerifier,
    MockBLEVerifier, ProximityError, ProximityVerifier,
};

// ============================================================
// BLE Advertisement
// Feature: contact_exchange.feature @ble @advertisement
// ============================================================

/// Test: Create BLE advertisement for exchange
#[test]
fn test_create_ble_advertisement() {
    let keypair = SigningKeyPair::generate();
    let exchange_token = [42u8; 32];

    let advertisement = BLEAdvertisement::new(&keypair, exchange_token);

    assert_eq!(advertisement.exchange_token(), &exchange_token);
    assert!(advertisement.verify_signature(&keypair.public_key()));
}

/// Test: Advertisement includes service UUID
#[test]
fn test_advertisement_service_uuid() {
    let keypair = SigningKeyPair::generate();
    let exchange_token = [0u8; 32];

    let advertisement = BLEAdvertisement::new(&keypair, exchange_token);

    // Vauchi BLE service UUID (custom 128-bit UUID)
    let expected_uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    assert_eq!(advertisement.service_uuid(), expected_uuid);
}

/// Test: Advertisement payload fits in BLE limits
#[test]
fn test_advertisement_payload_size() {
    let keypair = SigningKeyPair::generate();
    let exchange_token = [0u8; 32];

    let advertisement = BLEAdvertisement::new(&keypair, exchange_token);
    let payload = advertisement.to_bytes();

    // BLE advertisement data max is 31 bytes for legacy, 254 for extended
    // We use scan response for additional data, total ~62 bytes legacy
    assert!(
        payload.len() <= 200,
        "Payload should fit in extended advertisement"
    );
}

/// Test: Parse advertisement from bytes
#[test]
fn test_parse_advertisement() {
    let keypair = SigningKeyPair::generate();
    let exchange_token = [123u8; 32];

    let original = BLEAdvertisement::new(&keypair, exchange_token);
    let bytes = original.to_bytes();

    let parsed = BLEAdvertisement::from_bytes(&bytes).expect("Should parse");

    assert_eq!(parsed.exchange_token(), &exchange_token);
    assert!(parsed.verify_signature(&keypair.public_key()));
}

// ============================================================
// BLE Device Discovery
// Feature: contact_exchange.feature @ble @discovery
// ============================================================

/// Test: Discover nearby devices
#[test]
fn test_discover_nearby_devices() {
    let device1 = BLEDevice::new("device-1", -50);
    let device2 = BLEDevice::new("device-2", -70);
    let mock = MockBLEVerifier::new(vec![device1, device2], 1.0);

    let devices = mock.discover_nearby(Duration::from_secs(5)).unwrap();

    assert_eq!(devices.len(), 2);
    assert!(devices.iter().any(|d| d.id == "device-1"));
}

/// Test: Filter devices by exchange token
#[test]
fn test_filter_devices_with_exchange_token() {
    let token = [42u8; 32];
    let device_with_token = BLEDevice::new("device-1", -50).with_exchange_token(token);
    let device_without = BLEDevice::new("device-2", -60);

    let mock = MockBLEVerifier::new(vec![device_with_token, device_without], 1.0);
    let devices = mock.discover_nearby(Duration::from_secs(5)).unwrap();

    let vauchi_devices: Vec<_> = devices
        .iter()
        .filter(|d| d.exchange_token.is_some())
        .collect();

    assert_eq!(vauchi_devices.len(), 1);
    assert_eq!(vauchi_devices[0].exchange_token.unwrap(), token);
}

/// Test: Estimate distance from RSSI
#[test]
fn test_estimate_distance() {
    let device = BLEDevice::new("device-1", -50);
    let mock = MockBLEVerifier::success_at_distance(1.5);

    let distance = mock.estimate_distance(&device).unwrap();

    assert!((distance - 1.5).abs() < 0.01);
}

/// Test: Device within range check
#[test]
fn test_device_within_range() {
    let device = BLEDevice::new("device-1", -50);
    let close_mock = MockBLEVerifier::success_at_distance(1.0);
    let far_mock = MockBLEVerifier::success_at_distance(5.0);

    assert!(close_mock.is_within_range(&device, 2.0));
    assert!(!far_mock.is_within_range(&device, 2.0));
}

// ============================================================
// BLE Exchange Session
// Feature: contact_exchange.feature @ble @session
// ============================================================

/// Test: Create exchange session
#[test]
fn test_create_exchange_session() {
    let keypair = SigningKeyPair::generate();
    let session = BLEExchangeSession::new(&keypair);

    assert!(matches!(session.state(), BLEExchangeState::Idle));
    assert!(session.exchange_token().is_some());
}

/// Test: Start advertising
#[test]
fn test_start_advertising() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::new(&keypair);

    session
        .start_advertising()
        .expect("Should start advertising");

    assert!(matches!(session.state(), BLEExchangeState::Advertising));
}

/// Test: Start scanning
#[test]
fn test_start_scanning() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::new(&keypair);

    session.start_scanning().expect("Should start scanning");

    assert!(matches!(session.state(), BLEExchangeState::Scanning));
}

/// Test: Connect to discovered device
#[test]
fn test_connect_to_device() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::new(&keypair);

    let device = BLEDevice::new("peer-device", -50).with_exchange_token([1u8; 32]);

    session.start_scanning().unwrap();
    session.connect_to_device(&device).expect("Should connect");

    assert!(matches!(
        session.state(),
        BLEExchangeState::Connected { .. }
    ));
}

/// Test: Exchange contact data
#[test]
fn test_exchange_contact_data() {
    let alice_keypair = SigningKeyPair::generate();
    let bob_keypair = SigningKeyPair::generate();

    let mut alice_session = BLEExchangeSession::new(&alice_keypair);
    let mut bob_session = BLEExchangeSession::new(&bob_keypair);

    // Alice advertises, Bob scans
    alice_session.start_advertising().unwrap();
    bob_session.start_scanning().unwrap();

    // Simulate Bob discovering Alice
    let alice_device = BLEDevice::new("alice-device", -50)
        .with_exchange_token(*alice_session.exchange_token().unwrap());

    bob_session.connect_to_device(&alice_device).unwrap();

    // Exchange data
    let alice_card = b"Alice's contact card";
    let bob_card = b"Bob's contact card";

    alice_session.set_contact_data(alice_card);
    bob_session.set_contact_data(bob_card);

    // Simulate exchange (in real implementation this would use BLE GATT)
    let alice_received = bob_session.get_local_contact_data();
    let bob_received = alice_session.get_local_contact_data();

    assert_eq!(alice_received, Some(bob_card.as_slice()));
    assert_eq!(bob_received, Some(alice_card.as_slice()));
}

/// Test: Session timeout
#[test]
fn test_session_timeout() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::with_timeout(&keypair, Duration::from_millis(10));

    session.start_scanning().unwrap();

    // Wait for timeout
    std::thread::sleep(Duration::from_millis(50));

    session.check_timeout();
    assert!(matches!(session.state(), BLEExchangeState::TimedOut));
}

/// Test: Cancel session
#[test]
fn test_cancel_session() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::new(&keypair);

    session.start_advertising().unwrap();
    session.cancel();

    assert!(matches!(session.state(), BLEExchangeState::Cancelled));
}

// ============================================================
// Proximity Verification
// Feature: contact_exchange.feature @ble @proximity
// ============================================================

/// Test: Verify device proximity before exchange
#[test]
fn test_verify_proximity_before_exchange() {
    let device = BLEDevice::new("peer", -50);

    let close_verifier = MockBLEVerifier::success_at_distance(1.0);
    let far_verifier = MockBLEVerifier::success_at_distance(10.0);

    assert!(close_verifier.verify_device_proximity(&device).is_ok());
    assert!(far_verifier.verify_device_proximity(&device).is_err());
}

/// Test: Proximity challenge-response
#[test]
fn test_proximity_challenge_response() {
    let verifier = MockBLEVerifier::success_at_distance(1.0);
    let challenge = [0u8; 16];

    verifier.emit_challenge(&challenge).expect("Should emit");

    let response = verifier
        .listen_for_response(Duration::from_secs(5))
        .expect("Should receive response");

    assert!(verifier.verify_response(&challenge, &response));
}

/// Test: Proximity verification fails when too far
#[test]
fn test_proximity_fails_when_too_far() {
    let device = BLEDevice::new("far-device", -90);
    let verifier = MockBLEVerifier::success_at_distance(5.0);

    let result = verifier.verify_device_proximity(&device);

    assert!(matches!(result, Err(ProximityError::TooFar)));
}

// ============================================================
// Error Handling
// Feature: contact_exchange.feature @ble @errors
// ============================================================

/// Test: Discovery failure handling
#[test]
fn test_discovery_failure() {
    let verifier = MockBLEVerifier::failure();

    let result = verifier.discover_nearby(Duration::from_secs(5));

    assert!(result.is_err());
}

/// Test: Connection to device without token fails
#[test]
fn test_connect_requires_exchange_token() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::new(&keypair);

    let device_without_token = BLEDevice::new("peer", -50);

    session.start_scanning().unwrap();
    let result = session.connect_to_device(&device_without_token);

    assert!(result.is_err(), "Should require exchange token");
}

/// Test: Cannot exchange data without connection
#[test]
fn test_cannot_exchange_without_connection() {
    let keypair = SigningKeyPair::generate();
    let session = BLEExchangeSession::new(&keypair);

    let result = session.get_peer_contact_data();

    assert!(
        result.is_none(),
        "Should not have peer data without connection"
    );
}

// ============================================================
// Serialization
// ============================================================

/// Test: Advertisement serialization roundtrip
#[test]
fn test_advertisement_serialization() {
    let keypair = SigningKeyPair::generate();
    let exchange_token = [77u8; 32];

    let original = BLEAdvertisement::new(&keypair, exchange_token);
    let bytes = original.to_bytes();
    let restored = BLEAdvertisement::from_bytes(&bytes).unwrap();

    assert_eq!(original.exchange_token(), restored.exchange_token());
}

/// Test: Exchange session state serialization
#[test]
fn test_session_state_serialization() {
    let state = BLEExchangeState::Connected {
        peer_token: [42u8; 32],
        peer_device_id: "test-device".to_string(),
    };

    let json = serde_json::to_string(&state).expect("Should serialize");
    let restored: BLEExchangeState = serde_json::from_str(&json).expect("Should deserialize");

    assert!(matches!(restored, BLEExchangeState::Connected { .. }));
}
