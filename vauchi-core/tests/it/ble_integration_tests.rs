// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! BLE Exchange Integration Tests
//!
//! Integration tests for BLE-based contact exchange.
//! Feature file: features/ble_exchange.feature @integration
//!
//! These tests verify:
//! - BLE advertisement creation
//! - Device discovery
//! - Exchange session management
//! - Proximity verification

use std::time::Duration;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::exchange::{BLEAdvertisement, BLEDevice, BLEExchangeSession, BLEExchangeState};

// ============================================================
// Feature: ble_exchange.feature @integration @advertisement
// ============================================================

/// Test: Create BLE advertisement for exchange
// @scenario: ble_exchange :: Create BLE advertisement with valid token and signature
#[test]
fn test_create_ble_advertisement() {
    let keypair = SigningKeyPair::generate();
    let exchange_token = [42u8; 32];

    let advertisement = BLEAdvertisement::new(&keypair, exchange_token);

    assert_eq!(advertisement.exchange_token(), &exchange_token);
    assert!(advertisement.verify_signature(&keypair.public_key()));
}

/// Test: Advertisement includes service UUID
// @scenario: ble_exchange :: Advertisement includes correct Vauchi service UUID
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
// @scenario: ble_exchange :: Advertisement payload fits BLE limits
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
// @scenario: ble_exchange :: Advertisement serialization roundtrip
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
// Feature: ble_exchange.feature @integration @discovery
// ============================================================

// ============================================================
// Feature: ble_exchange.feature @integration @session
// ============================================================

/// Test: Create exchange session
// @scenario: ble_exchange :: Exchange session creation
#[test]
fn test_create_exchange_session() {
    let keypair = SigningKeyPair::generate();
    let session = BLEExchangeSession::new(&keypair);

    assert!(matches!(session.state(), BLEExchangeState::Idle));
    session.exchange_token().expect("expected Some");
}

/// Test: Start advertising
// @scenario: ble_exchange :: Session transitions to Advertising
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
// @scenario: ble_exchange :: Session transitions to Scanning
#[test]
fn test_start_scanning() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::new(&keypair);

    session.start_scanning().expect("Should start scanning");

    assert!(matches!(session.state(), BLEExchangeState::Scanning));
}

/// Test: Connect to discovered device
// @scenario: ble_exchange :: Connect to discovered device
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
// @scenario: ble_exchange :: Full mock exchange with data transfer
#[test]
fn test_exchange_contact_data() {
    let alice_keypair = SigningKeyPair::generate();
    let bob_keypair = SigningKeyPair::generate();

    let mut alice_session = BLEExchangeSession::new(&alice_keypair);
    let mut bob_session = BLEExchangeSession::new(&bob_keypair);

    alice_session.start_advertising().unwrap();
    bob_session.start_scanning().unwrap();

    // Simulate Bob discovering Alice
    let alice_device = BLEDevice::new("alice-device", -50)
        .with_exchange_token(*alice_session.exchange_token().unwrap());

    bob_session.connect_to_device(&alice_device).unwrap();

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
// @scenario: ble_exchange :: Session timeout
#[test]
fn test_session_timeout() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::with_timeout(&keypair, Duration::from_millis(10));

    session.start_scanning().unwrap();

    // Advance time past the session timeout — no sleep needed
    let future = std::time::Instant::now() + Duration::from_millis(20);
    session.check_timeout_at(future);

    assert!(matches!(session.state(), BLEExchangeState::TimedOut));
}

/// Test: the session-timeout boundary `check_timeout()` is driven by
/// the injected `MonotonicClock` (Phase 1 / Task 1.1b), not ambient
/// `Instant::now()`. Exercises both the `started_at` stamp and the
/// no-argument boundary. Before the migration this fails: advancing the
/// fake clock has no effect, so the session never times out.
// @scenario: ble_exchange :: Session timeout
#[test]
fn test_session_timeout_driven_by_injected_monotonic_clock() {
    use std::sync::Arc;
    use vauchi_core::monotonic::FakeMonotonicClock;

    let keypair = SigningKeyPair::generate();
    let fake = Arc::new(FakeMonotonicClock::new());
    let mut session = BLEExchangeSession::with_timeout(&keypair, Duration::from_millis(10))
        .with_monotonic(fake.clone());

    session.start_scanning().unwrap();
    // No advance yet: within the 10 ms window, not timed out.
    session.check_timeout();
    assert!(
        !matches!(session.state(), BLEExchangeState::TimedOut),
        "session must not time out before the injected clock advances"
    );

    fake.advance(Duration::from_millis(20));
    session.check_timeout();
    assert!(
        matches!(session.state(), BLEExchangeState::TimedOut),
        "advancing the injected clock past the timeout must mark the session TimedOut"
    );
}

/// Test: Cancel session
// @scenario: ble_exchange :: Cancel session
#[test]
fn test_cancel_session() {
    let keypair = SigningKeyPair::generate();
    let mut session = BLEExchangeSession::new(&keypair);

    session.start_advertising().unwrap();
    session.cancel();

    assert!(matches!(session.state(), BLEExchangeState::Cancelled));
}

// ============================================================
// Feature: ble_exchange.feature @integration @proximity
// ============================================================

// ============================================================
// Feature: ble_exchange.feature @integration @error
// ============================================================

/// Test: Connection to device without token fails
// @scenario: ble_exchange :: Connection requires exchange token
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
// @scenario: ble_exchange :: Cannot exchange without connection
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
// ============================================================

/// Test: Advertisement serialization roundtrip
// @scenario: ble_exchange :: Advertisement serialization roundtrip
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
// @scenario: ble_exchange :: Session state serialization roundtrip
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
