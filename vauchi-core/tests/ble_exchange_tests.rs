// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for BLE Exchange (Feature C)
//!
//! Feature file: features/ble_exchange.feature @ble
//!
//! GATT-based payload exchange with proximity verification.
//! Both sides use fresh ephemeral X25519 keys for full forward secrecy.

use vauchi_core::exchange::{
    BLETransport, ExchangeBle, ExchangeError, ExchangeEvent, ExchangeSession, ExchangeState,
    ExchangeTransport, MockBLETransport, MockProximityVerifier, X3DHKeyPair, BLE_PAYLOAD_SIZE,
    CHAR_CARD_EXCHANGE, CHAR_CHALLENGE, CHAR_EXCHANGE_PAYLOAD, VAUCHI_BLE_SERVICE_UUID,
};
use vauchi_core::{ContactCard, Identity};

// ============================================================
// ExchangeBle payload
// ============================================================

// @scenario: ble_exchange.feature:BLE payload generation contains identity and exchange keys
#[test]
fn test_ble_payload_generate() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(&identity, &ephemeral);

    assert_eq!(payload.identity_key(), identity.signing_public_key());
    assert_eq!(payload.exchange_key(), ephemeral.public_key());
    assert!(!payload.is_expired());
    assert!(payload.verify_signature());
}

// @scenario: ble_exchange.feature:BLE payload serialization roundtrip
#[test]
fn test_ble_payload_roundtrip() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(&identity, &ephemeral);
    let bytes = payload.to_bytes();

    assert_eq!(bytes.len(), BLE_PAYLOAD_SIZE);
    assert_eq!(&bytes[0..4], b"VBLE");

    let parsed = ExchangeBle::from_bytes(&bytes).expect("Should parse valid payload");

    assert_eq!(parsed.identity_key(), payload.identity_key());
    assert_eq!(parsed.exchange_key(), payload.exchange_key());
    assert_eq!(parsed.token(), payload.token());
    assert_eq!(parsed.timestamp(), payload.timestamp());
    assert!(parsed.verify_signature());
}

// @scenario: security.feature:Tampered exchange data is rejected
#[test]
fn test_ble_payload_signature() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(&identity, &ephemeral);
    assert!(payload.verify_signature());
}

// @scenario: security.feature:Tampered exchange data is rejected
#[test]
fn test_ble_payload_tamper() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(&identity, &ephemeral);
    let mut bytes = payload.to_bytes();

    // Tamper with exchange key byte
    bytes[40] ^= 0xFF;

    let parsed = ExchangeBle::from_bytes(&bytes).unwrap();
    assert!(
        !parsed.verify_signature(),
        "Tampered BLE payload should fail signature"
    );
}

// @scenario: ble_exchange.feature:BLE payload rejected with invalid magic bytes
#[test]
fn test_ble_payload_magic() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(&identity, &ephemeral);
    let mut bytes = payload.to_bytes();

    bytes[0..4].copy_from_slice(b"XXXX");

    let result = ExchangeBle::from_bytes(&bytes);
    assert!(matches!(result, Err(ExchangeError::InvalidBleFormat)));
}

// @scenario: ble_exchange.feature:BLE payload expires after 60 seconds
#[test]
fn test_ble_payload_expiry() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 2 minutes ago — should be expired (BLE is 60s)
    let old = ExchangeBle::generate_with_timestamp(&identity, &ephemeral, [0u8; 32], now - 120);
    assert!(old.is_expired(), "2-min old BLE payload should be expired");

    // Fresh
    let fresh = ExchangeBle::generate_with_timestamp(&identity, &ephemeral, [0u8; 32], now);
    assert!(
        !fresh.is_expired(),
        "Fresh BLE payload should not be expired"
    );
}

// ============================================================
// GATT constants
// ============================================================

// @scenario: ble_exchange.feature:GATT UUIDs have correct format
#[test]
fn test_gatt_uuid_validity() {
    // UUIDs should be valid format: 8-4-4-4-12
    let uuids = [
        VAUCHI_BLE_SERVICE_UUID,
        CHAR_EXCHANGE_PAYLOAD,
        CHAR_CARD_EXCHANGE,
        CHAR_CHALLENGE,
    ];

    for uuid in &uuids {
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5, "UUID {} should have 5 parts", uuid);
    }
}

// @scenario: ble_exchange.feature:GATT service and characteristic UUIDs match expected values
#[test]
fn test_gatt_service_uuid_match() {
    // Service UUID should match the constant
    assert_eq!(
        VAUCHI_BLE_SERVICE_UUID,
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    );

    // Characteristic UUIDs should share the same base but differ in last 4 digits
    assert!(CHAR_EXCHANGE_PAYLOAD.ends_with("7891"));
    assert!(CHAR_CARD_EXCHANGE.ends_with("7892"));
    assert!(CHAR_CHALLENGE.ends_with("7893"));
}

// ============================================================
// BLETransport trait + MockBLETransport
// ============================================================

// @scenario: ble_exchange.feature:BLE transport can advertise
#[test]
fn test_mock_ble_transport_advertise() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();
    let payload = ExchangeBle::generate(&identity, &ephemeral);

    let mock = MockBLETransport::with_peer_payload(&payload.to_bytes());
    assert!(mock.start_advertising(&payload).is_ok());
}

// @scenario: ble_exchange.feature:BLE transport can scan
#[test]
fn test_mock_ble_transport_scan() {
    let mock = MockBLETransport::with_peer_payload(&[0u8; BLE_PAYLOAD_SIZE]);
    assert!(mock.start_scanning().is_ok());
}

// @scenario: ble_exchange.feature:BLE transport connect, read, write, disconnect
#[test]
fn test_mock_ble_transport_connect_read_write() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();
    let payload = ExchangeBle::generate(&identity, &ephemeral);
    let payload_bytes = payload.to_bytes();

    let mock = MockBLETransport::with_peer_payload(&payload_bytes);

    // Connect
    assert!(mock.connect("device-1").is_ok());

    // Read exchange payload
    let read = mock.read_characteristic(CHAR_EXCHANGE_PAYLOAD).unwrap();
    assert_eq!(read, payload_bytes);

    // Write card exchange data
    assert!(mock
        .write_characteristic(CHAR_CARD_EXCHANGE, b"encrypted-card")
        .is_ok());

    let written = mock.get_written();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, CHAR_CARD_EXCHANGE);

    // Disconnect
    assert!(mock.disconnect().is_ok());
}

// @scenario: ble_exchange.feature:BLE transport in failure mode returns errors
#[test]
fn test_mock_ble_transport_failure() {
    let mock = MockBLETransport::failing();

    assert!(mock.start_scanning().is_err());
    assert!(mock.connect("device-1").is_err());
    assert!(mock.read_characteristic(CHAR_EXCHANGE_PAYLOAD).is_err());
}

// ============================================================
// Session integration
// ============================================================

// @scenario: ble_exchange.feature:New BLE session starts in AwaitingBleConnection
#[test]
fn test_ble_session_starts_awaiting_connection() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_ble(identity, card, proximity);

    assert!(
        matches!(session.state(), ExchangeState::AwaitingBleConnection),
        "BLE session should start in AwaitingBleConnection"
    );
    assert_eq!(session.transport(), ExchangeTransport::Ble);
}

// @scenario: ble_exchange.feature:Session transitions to AwaitingBleVerification after payload exchange
#[test]
fn test_ble_payload_exchanged_transitions() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_ble(alice_identity, alice_card, proximity);

    // Bob generates his BLE payload
    let bob_eph = X3DHKeyPair::generate();
    let bob_payload = ExchangeBle::generate(&bob_identity, &bob_eph);

    // Alice receives Bob's payload via BLE
    alice_session
        .apply(ExchangeEvent::BlePayloadExchanged {
            their_payload: bob_payload.to_bytes().to_vec(),
            device_id: "bob-device-123".to_string(),
        })
        .expect("Should transition to AwaitingBleVerification");

    assert!(
        matches!(
            alice_session.state(),
            ExchangeState::AwaitingBleVerification { .. }
        ),
        "Should be in AwaitingBleVerification"
    );
}

// @scenario: ble_exchange.feature:Session transitions to AwaitingKeyAgreement after proximity verification
#[test]
fn test_ble_proximity_verified_transitions() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_ble(alice_identity, alice_card, proximity);

    // Payload exchange
    let bob_eph = X3DHKeyPair::generate();
    let bob_payload = ExchangeBle::generate(&bob_identity, &bob_eph);
    alice_session
        .apply(ExchangeEvent::BlePayloadExchanged {
            their_payload: bob_payload.to_bytes().to_vec(),
            device_id: "bob-device".to_string(),
        })
        .unwrap();

    // Proximity verification
    alice_session
        .apply(ExchangeEvent::BleProximityVerified)
        .expect("Should transition to AwaitingKeyAgreement");

    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));
}

// @scenario: ble_exchange.feature:Full BLE exchange lifecycle
// @scenario: ble_exchange.feature:Symmetric DH produces identical shared keys
#[test]
fn test_ble_full_lifecycle() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_ble(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
    );
    let mut bob_session = ExchangeSession::new_ble(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
    );

    // Generate payloads (in real life, built from session's identity+ephemeral)
    let alice_id2 = Identity::create("Alice");
    let bob_id2 = Identity::create("Bob");
    let alice_eph = X3DHKeyPair::generate();
    let bob_eph = X3DHKeyPair::generate();

    let alice_ble = ExchangeBle::generate(&alice_id2, &alice_eph);
    let bob_ble = ExchangeBle::generate(&bob_id2, &bob_eph);

    // Step 1: Payload exchange via GATT
    alice_session
        .apply(ExchangeEvent::BlePayloadExchanged {
            their_payload: bob_ble.to_bytes().to_vec(),
            device_id: "bob-device".to_string(),
        })
        .unwrap();
    bob_session
        .apply(ExchangeEvent::BlePayloadExchanged {
            their_payload: alice_ble.to_bytes().to_vec(),
            device_id: "alice-device".to_string(),
        })
        .unwrap();

    // Step 2: Proximity verification (challenge-response)
    alice_session
        .apply(ExchangeEvent::BleProximityVerified)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::BleProximityVerified)
        .unwrap();

    // Step 3: Key agreement (symmetric DH)
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Step 4: Card exchange
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    // Both complete
    assert!(matches!(
        alice_session.state(),
        ExchangeState::Complete { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::Complete { .. }
    ));
}

// @scenario: ble_exchange.feature:Symmetric DH produces identical shared keys
#[test]
fn test_ble_shared_keys_match() {
    // Verify symmetric DH produces matching keys
    let alice_eph = X3DHKeyPair::generate();
    let bob_eph = X3DHKeyPair::generate();

    let alice_shared = alice_eph.diffie_hellman(bob_eph.public_key()).unwrap();
    let bob_shared = bob_eph.diffie_hellman(alice_eph.public_key()).unwrap();

    assert_eq!(alice_shared, bob_shared, "BLE symmetric DH should match");
}

// @scenario: ble_exchange.feature:Expired BLE payload is rejected
#[test]
fn test_ble_expired_rejection() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_ble(alice_identity, alice_card, proximity);

    let bob_eph = X3DHKeyPair::generate();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expired =
        ExchangeBle::generate_with_timestamp(&bob_identity, &bob_eph, [0u8; 32], now - 120);

    let result = alice_session.apply(ExchangeEvent::BlePayloadExchanged {
        their_payload: expired.to_bytes().to_vec(),
        device_id: "bob-device".to_string(),
    });
    assert!(
        matches!(result, Err(ExchangeError::BleExpired)),
        "Expired BLE payload should be rejected"
    );
}

// @scenario: ble_exchange.feature:Self-exchange is rejected via BLE
#[test]
fn test_ble_self_exchange() {
    let alice_identity = Identity::create("Alice");

    let eph = X3DHKeyPair::generate();
    let self_payload = ExchangeBle::generate(&alice_identity, &eph);
    let self_bytes = self_payload.to_bytes().to_vec();

    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_ble(alice_identity, card, proximity);

    let result = session.apply(ExchangeEvent::BlePayloadExchanged {
        their_payload: self_bytes,
        device_id: "self-device".to_string(),
    });
    assert!(
        matches!(result, Err(ExchangeError::SelfExchange)),
        "BLE self-exchange should be rejected"
    );
}

// @scenario: ble_exchange.feature:Invalid BLE payload is rejected
#[test]
fn test_ble_invalid_payload() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_ble(identity, card, proximity);

    let result = session.apply(ExchangeEvent::BlePayloadExchanged {
        their_payload: vec![0u8; 174],
        device_id: "device".to_string(),
    });
    assert!(
        matches!(result, Err(ExchangeError::InvalidBleFormat)),
        "Invalid BLE payload should be rejected"
    );
}

// @scenario: ble_exchange.feature:BLE events rejected on non-BLE transport
#[test]
fn test_ble_rejects_wrong_transport() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    // QR session
    let mut session = ExchangeSession::new_qr(identity, card, proximity);

    let result = session.apply(ExchangeEvent::BlePayloadExchanged {
        their_payload: vec![],
        device_id: "device".to_string(),
    });
    assert!(
        result.is_err(),
        "BLE events should be rejected on non-BLE transport"
    );
}

// ============================================================
// Challenge-response
// ============================================================

// @scenario: ble_exchange.feature:Proximity verification requires AwaitingBleVerification state
#[test]
fn test_ble_proximity_requires_verification_state() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_ble(identity, card, proximity);

    // Try to verify proximity without payload exchange
    let result = session.apply(ExchangeEvent::BleProximityVerified);
    assert!(
        result.is_err(),
        "Should fail when not in AwaitingBleVerification state"
    );
}

// @scenario: ble_exchange.feature:Key agreement blocked without proximity verification
#[test]
fn test_ble_challenge_response_blocks_on_failure() {
    // If proximity verification fails, the exchange should not proceed.
    // We verify this by checking state doesn't advance past AwaitingBleVerification
    // if BleProximityVerified is not sent.
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_ble(alice_identity, alice_card, proximity);

    let bob_eph = X3DHKeyPair::generate();
    let bob_payload = ExchangeBle::generate(&bob_identity, &bob_eph);

    session
        .apply(ExchangeEvent::BlePayloadExchanged {
            their_payload: bob_payload.to_bytes().to_vec(),
            device_id: "device".to_string(),
        })
        .unwrap();

    // In AwaitingBleVerification — cannot proceed to key agreement directly
    let result = session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        result.is_err(),
        "Should not be able to do key agreement without proximity verification"
    );
}

// ============================================================
// Full lifecycle with mock transport
// ============================================================

// @scenario: ble_exchange.feature:Full exchange with mock transport
// @scenario: ble_exchange.feature:Symmetric DH produces identical shared keys
#[test]
fn test_ble_full_exchange_with_mock_transport() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_eph = X3DHKeyPair::generate();
    let bob_eph = X3DHKeyPair::generate();

    let alice_ble = ExchangeBle::generate(&alice_identity, &alice_eph);
    let bob_ble = ExchangeBle::generate(&bob_identity, &bob_eph);

    // Alice's mock transport has Bob's payload available to read
    let alice_transport = MockBLETransport::with_peer_payload(&bob_ble.to_bytes());
    // Bob's mock transport has Alice's payload available to read
    let bob_transport = MockBLETransport::with_peer_payload(&alice_ble.to_bytes());

    // Alice's side: scan, connect, read Bob's payload
    alice_transport.start_scanning().unwrap();
    alice_transport.connect("bob-device").unwrap();
    let bob_payload_bytes = alice_transport
        .read_characteristic(CHAR_EXCHANGE_PAYLOAD)
        .unwrap();

    // Bob's side: advertise, read Alice's payload
    bob_transport.start_advertising(&bob_ble).unwrap();
    let alice_payload_bytes = bob_transport
        .read_characteristic(CHAR_EXCHANGE_PAYLOAD)
        .unwrap();

    // Parse payloads
    let bob_received = ExchangeBle::from_bytes(&bob_payload_bytes).unwrap();
    let alice_received = ExchangeBle::from_bytes(&alice_payload_bytes).unwrap();

    assert!(bob_received.verify_signature());
    assert!(alice_received.verify_signature());

    // Symmetric DH
    let alice_shared = alice_eph
        .diffie_hellman(bob_received.exchange_key())
        .unwrap();
    let bob_shared = bob_eph
        .diffie_hellman(alice_received.exchange_key())
        .unwrap();

    assert_eq!(
        alice_shared, bob_shared,
        "BLE exchange should produce matching keys"
    );

    // Write card data via GATT
    alice_transport
        .write_characteristic(CHAR_CARD_EXCHANGE, b"alice-encrypted-card")
        .unwrap();
    bob_transport
        .write_characteristic(CHAR_CARD_EXCHANGE, b"bob-encrypted-card")
        .unwrap();

    // Disconnect
    alice_transport.disconnect().unwrap();
    bob_transport.disconnect().unwrap();

    // Verify written data
    let alice_written = alice_transport.get_written();
    assert_eq!(alice_written.len(), 1);
    assert_eq!(alice_written[0].0, CHAR_CARD_EXCHANGE);
}

// ============================================================
// BLE error variants
// ============================================================

// @scenario: ble_exchange.feature:BLE error variants have proper display messages
#[test]
fn test_ble_error_variants_exist() {
    let err1 = ExchangeError::InvalidBleFormat;
    let err2 = ExchangeError::BleExpired;
    let err3 = ExchangeError::BleOutOfRange;
    let err4 = ExchangeError::BleConnectionLost;
    let err5 = ExchangeError::BleNotAvailable;

    assert_eq!(format!("{}", err1), "Invalid BLE payload format");
    assert_eq!(format!("{}", err2), "BLE payload has expired");
    assert_eq!(format!("{}", err3), "BLE device out of range");
    assert_eq!(format!("{}", err4), "BLE connection lost during exchange");
    assert_eq!(format!("{}", err5), "BLE not available on this device");
}

// @scenario: ble_exchange.feature:BLE payload is exactly 174 bytes
#[test]
fn test_ble_payload_size() {
    assert_eq!(BLE_PAYLOAD_SIZE, 174, "BLE payload should be 174 bytes");

    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();
    let payload = ExchangeBle::generate(&identity, &ephemeral);
    let bytes = payload.to_bytes();

    assert_eq!(bytes.len(), 174);
}

// @scenario: ble_exchange.feature:BLE ephemeral keys differ from identity keys
// @scenario: contact_exchange.feature:BLE exchange uses fresh ephemeral keys
// @scenario: security.feature:Forward secrecy via Double Ratchet
#[test]
fn test_ble_forward_secrecy() {
    // Both sides use fresh ephemeral keys — not identity-derived
    let identity = Identity::create("Alice");

    let identity_x3dh = identity.x3dh_keypair();
    let ble_eph = X3DHKeyPair::generate();

    assert_ne!(
        ble_eph.public_key(),
        identity_x3dh.public_key(),
        "BLE ephemeral should be different from identity exchange key"
    );
}
