// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tests for BLE Exchange (Feature C)
//!
//! Feature file: features/ble_exchange.feature @ble
//!
//! The BLE payload format, GATT identifiers and BLE error variants.
//! Both sides use fresh ephemeral X25519 keys for full forward secrecy.
//!
//! The BLE handshake itself runs in `BleHandshakeSession`
//! (`ble_handshake_tests.rs`); `ExchangeSession` refuses BLE events
//! (vauchi/private#893).

use vauchi_core::exchange::{
    BLE_PAYLOAD_SIZE, CHAR_CARD_EXCHANGE, CHAR_CHALLENGE, CHAR_EXCHANGE_PAYLOAD, ExchangeBle,
    ExchangeError, ExchangeEvent, ExchangeSession, ExchangeState, MockProximityVerifier,
    VAUCHI_BLE_SERVICE_UUID, X3DHKeyPair,
};
use vauchi_core::{ContactCard, Identity};

// ============================================================
// ============================================================

// @scenario: ble_exchange :: BLE payload generation contains identity and exchange keys
#[test]
fn test_ble_payload_generate() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    assert_eq!(payload.identity_key(), identity.signing_public_key());
    assert_eq!(payload.exchange_key(), ephemeral.public_key());
    assert!(!payload.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()));
    assert!(payload.verify_signature());
}

// @scenario: ble_exchange :: BLE payload serialization roundtrip
#[test]
fn test_ble_payload_roundtrip() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
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

// @scenario: security :: Tampered exchange data is rejected
// @scenario: ble_exchange :: BLE payload has valid signature
#[test]
fn test_ble_payload_signature() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(payload.verify_signature());
}

// @scenario: security :: Tampered exchange data is rejected
// @scenario: ble_exchange :: Tampered BLE payload fails signature verification
#[test]
fn test_ble_payload_tamper() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bytes = payload.to_bytes();

    // Tamper with exchange key byte
    bytes[40] ^= 0xFF;

    let parsed = ExchangeBle::from_bytes(&bytes).unwrap();
    assert!(
        !parsed.verify_signature(),
        "Tampered BLE payload should fail signature"
    );
}

// @scenario: ble_exchange :: BLE payload rejected with invalid magic bytes
#[test]
fn test_ble_payload_magic() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeBle::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bytes = payload.to_bytes();

    bytes[0..4].copy_from_slice(b"XXXX");

    let result = ExchangeBle::from_bytes(&bytes);
    assert!(matches!(result, Err(ExchangeError::InvalidBleFormat)));
}

// @scenario: ble_exchange :: BLE payload expires after 60 seconds
#[test]
fn test_ble_payload_expiry() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 2 minutes ago — should be expired (BLE is 60s)
    let old = ExchangeBle::generate_with_timestamp(&identity, &ephemeral, [0u8; 32], now - 120);
    assert!(
        old.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()),
        "2-min old BLE payload should be expired"
    );

    let fresh = ExchangeBle::generate_with_timestamp(&identity, &ephemeral, [0u8; 32], now);
    assert!(
        !fresh.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()),
        "Fresh BLE payload should not be expired"
    );
}

// ============================================================
// ============================================================

// @scenario: ble_exchange :: GATT UUIDs have correct format
#[test]
fn test_gatt_uuid_validity() {
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

// @scenario: ble_exchange :: GATT service and characteristic UUIDs match expected values
#[test]
fn test_gatt_service_uuid_match() {
    assert_eq!(
        VAUCHI_BLE_SERVICE_UUID,
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    );

    assert!(CHAR_EXCHANGE_PAYLOAD.ends_with("7891"));
    assert!(CHAR_CARD_EXCHANGE.ends_with("7892"));
    assert!(CHAR_CHALLENGE.ends_with("7893"));
}

// ============================================================
// ============================================================

// @scenario: ble_exchange :: Symmetric DH produces identical shared keys
#[test]
fn test_ble_shared_keys_match() {
    let alice_eph = X3DHKeyPair::generate();
    let bob_eph = X3DHKeyPair::generate();

    let alice_shared = alice_eph.diffie_hellman(bob_eph.public_key()).unwrap();
    let bob_shared = bob_eph.diffie_hellman(alice_eph.public_key()).unwrap();

    assert_eq!(alice_shared, bob_shared, "BLE symmetric DH should match");
}

// @scenario: ble_exchange :: BLE events rejected on non-BLE transport
#[test]
fn test_ble_rejects_wrong_transport() {
    let mut session = ExchangeSession::new_qr(
        Identity::create("Alice", 0),
        ContactCard::new("Alice"),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    let ble_events = [
        ExchangeEvent::StartBleExchange,
        ExchangeEvent::BlePayloadExchanged {
            their_payload: vec![],
            device_id: "device".to_string(),
        },
        ExchangeEvent::BleProximityVerified,
    ];
    for event in ble_events {
        let label = format!("{event:?}");
        let result = session.apply(event);
        assert!(
            matches!(result, Err(ExchangeError::InvalidState(_))),
            "{label} must be rejected on a QR session, got {result:?}"
        );
    }
    assert!(
        matches!(session.state(), ExchangeState::Idle),
        "rejected BLE events must leave the session untouched, got {:?}",
        session.state()
    );
}

// ============================================================
// ============================================================

// @scenario: ble_exchange :: BLE error variants have proper display messages
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

// @scenario: ble_exchange :: BLE payload is exactly 174 bytes
#[test]
fn test_ble_payload_size() {
    assert_eq!(BLE_PAYLOAD_SIZE, 174, "BLE payload should be 174 bytes");

    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let payload = ExchangeBle::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let bytes = payload.to_bytes();

    assert_eq!(bytes.len(), 174);
}

// @scenario: ble_exchange :: BLE ephemeral keys differ from identity keys
// @scenario: contact_exchange :: BLE exchange uses fresh ephemeral keys
// @scenario: security :: Forward secrecy via Double Ratchet
#[test]
fn test_ble_forward_secrecy() {
    // Both sides use fresh ephemeral keys — not identity-derived
    let identity = Identity::create("Alice", 0);

    let identity_x3dh = identity.x3dh_keypair();
    let ble_eph = X3DHKeyPair::generate();

    assert_ne!(
        ble_eph.public_key(),
        identity_x3dh.public_key(),
        "BLE ephemeral should be different from identity exchange key"
    );
}
