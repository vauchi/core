//! TDD Tests for Contact Exchange Protocol
//!
//! These tests are written FIRST (RED phase) before implementation.

use webbook_core::exchange::{ExchangeQR, X3DHKeyPair, X3DH};
use webbook_core::Identity;

// =============================================================================
// X3DH Key Agreement Tests
// =============================================================================

/// Tests that X3DH key agreement produces the same shared secret on both sides
#[test]
fn test_x3dh_key_agreement_produces_same_secret() {
    // Alice and Bob each have identity keys
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    // Alice initiates exchange with Bob's public key
    let (alice_secret, alice_ephemeral_public) =
        X3DH::initiate(&alice_keys, bob_keys.public_key()).expect("Key agreement should succeed");

    // Bob responds using Alice's ephemeral public key
    let bob_secret = X3DH::respond(&bob_keys, alice_keys.public_key(), &alice_ephemeral_public)
        .expect("Key agreement should succeed");

    // Both should derive the same shared secret
    assert_eq!(alice_secret.as_bytes(), bob_secret.as_bytes());
}

/// Tests that different key pairs produce different shared secrets
#[test]
fn test_x3dh_different_keys_different_secrets() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();
    let charlie = X3DHKeyPair::generate();

    // Alice-Bob exchange
    let (alice_bob_secret, _alice_ephemeral) = X3DH::initiate(&alice, bob.public_key()).unwrap();

    // Alice-Charlie exchange
    let (alice_charlie_secret, _) = X3DH::initiate(&alice, charlie.public_key()).unwrap();

    // Secrets should be different
    assert_ne!(alice_bob_secret.as_bytes(), alice_charlie_secret.as_bytes());
}

/// Tests that ephemeral keys are unique per session
#[test]
fn test_x3dh_ephemeral_keys_unique_per_session() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (_, ephemeral1) = X3DH::initiate(&alice, bob.public_key()).unwrap();
    let (_, ephemeral2) = X3DH::initiate(&alice, bob.public_key()).unwrap();

    // Each initiation should use a fresh ephemeral key
    assert_ne!(ephemeral1, ephemeral2);
}

/// Tests that shared secret can be used for encryption
#[test]
fn test_x3dh_shared_secret_usable_for_encryption() {
    use webbook_core::crypto::{decrypt, encrypt};

    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (alice_secret, ephemeral) = X3DH::initiate(&alice, bob.public_key()).unwrap();
    let bob_secret = X3DH::respond(&bob, alice.public_key(), &ephemeral).unwrap();

    // Alice encrypts a message
    let message = b"Hello Bob!";
    let ciphertext = encrypt(&alice_secret, message).expect("Encryption should succeed");

    // Bob decrypts with his derived key
    let decrypted = decrypt(&bob_secret, &ciphertext).expect("Decryption should succeed");
    assert_eq!(decrypted, message);
}

// =============================================================================
// QR Code Protocol Tests
// =============================================================================

/// Tests that QR code contains public key
#[test]
fn test_generate_qr_contains_public_key() {
    let identity = Identity::create("Alice");
    let qr = ExchangeQR::generate(&identity);

    assert_eq!(qr.public_key(), identity.signing_public_key());
}

/// Tests QR code roundtrip encode/decode
#[test]
fn test_qr_roundtrip_encode_decode() {
    let identity = Identity::create("Alice");
    let original = ExchangeQR::generate(&identity);

    let encoded = original.to_data_string();
    let decoded = ExchangeQR::from_data_string(&encoded).expect("Decoding should succeed");

    assert_eq!(original.public_key(), decoded.public_key());
    assert_eq!(original.exchange_token(), decoded.exchange_token());
}

/// Tests that QR code expires after 5 minutes
#[test]
fn test_qr_expires_after_5_minutes() {
    let identity = Identity::create("Alice");
    let qr = ExchangeQR::generate(&identity);

    // Fresh QR should not be expired
    assert!(!qr.is_expired());

    // Create a QR with timestamp 6 minutes in the past
    let old_qr = ExchangeQR::generate_with_timestamp(
        &identity,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 360, // 6 minutes ago
    );

    assert!(old_qr.is_expired());
}

/// Tests QR signature verification
#[test]
fn test_qr_signature_verification() {
    let identity = Identity::create("Alice");
    let qr = ExchangeQR::generate(&identity);

    assert!(qr.verify_signature());
}

/// Tests that malformed QR data is rejected
#[test]
fn test_malformed_qr_rejected() {
    let result = ExchangeQR::from_data_string("not-valid-qr-data");
    assert!(result.is_err());

    let result = ExchangeQR::from_data_string("");
    assert!(result.is_err());
}

/// Tests that QR from different app/protocol is rejected
#[test]
fn test_non_webbook_qr_rejected() {
    // Random base64 data that's not our protocol
    let fake_qr = "eyJub3QiOiJ3ZWJib29rIn0=";
    let result = ExchangeQR::from_data_string(fake_qr);
    assert!(result.is_err());
}

// =============================================================================
// BLE Proximity Tests (from contact_exchange.feature @ble scenarios)
// =============================================================================

use std::time::Duration;
use webbook_core::exchange::{BLEDevice, BLEProximityVerifier, MockBLEVerifier, ProximityError};

/// Feature: Contact Card Exchange
/// Scenario: Discover nearby WebBook users via BLE
/// Tests that BLE can discover nearby devices advertising WebBook
#[test]
fn test_ble_discover_nearby_webbook_users() {
    // Given Alice has BLE enabled
    // And Bob has BLE enabled and is within 2 meters
    let bob_device =
        BLEDevice::with_name("bob-device-uuid", "Bob's Phone", -50).with_exchange_token([42u8; 32]);
    let verifier = MockBLEVerifier::new(vec![bob_device], 1.5);

    // When Alice opens the "Nearby" screen (discovers devices)
    let discovered = verifier.discover_nearby(Duration::from_secs(5)).unwrap();

    // Then Alice should see Bob in the nearby users list
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].name.as_deref(), Some("Bob's Phone"));

    // And the signal strength should indicate close proximity
    assert!(discovered[0].rssi > -60); // Strong signal = close
}

/// Feature: Contact Card Exchange
/// Scenario: Initiate BLE exchange
/// Tests BLE exchange succeeds when devices are within 2 meters
#[test]
fn test_ble_exchange_succeeds_within_2_meters() {
    // Given Alice sees Bob in the nearby users list
    // And Bob is within 2 meters (verified by RSSI)
    let bob_device = BLEDevice::new("bob-uuid", -45);
    let verifier = MockBLEVerifier::new(vec![bob_device.clone()], 1.5); // 1.5 meters

    // When Alice taps on Bob to exchange
    // The proximity verification should pass
    let result = verifier.verify_device_proximity(&verifier.devices[0]);

    // Then contact cards should be exchanged
    assert!(result.is_ok());
}

/// Feature: Contact Card Exchange
/// Scenario: BLE exchange blocked when too far
/// Tests that exchange is blocked when devices are more than 2 meters apart
#[test]
fn test_ble_exchange_blocked_when_too_far() {
    // Given Alice sees Bob in the nearby users list
    // But Bob is more than 2 meters away
    let bob_device = BLEDevice::new("bob-uuid", -75); // Weak signal = far
    let verifier = MockBLEVerifier::new(vec![bob_device], 5.0); // 5 meters away

    // When Alice attempts to exchange with Bob
    let result = verifier.verify_device_proximity(&verifier.devices[0]);

    // Then the exchange should be blocked
    assert!(matches!(result, Err(ProximityError::TooFar)));
}

/// Feature: Contact Card Exchange
/// Scenario: BLE exchange with relay attack prevention
/// Tests that challenge-response detects relay attacks
#[test]
fn test_ble_relay_attack_detection() {
    // Given an attacker is relaying BLE signals
    // And Alice attempts to exchange with what appears to be Bob
    let fake_device = BLEDevice::new("relayed-uuid", -50);

    // Simulate relay attack - device appears close but fails challenge-response
    let mut verifier = MockBLEVerifier::new(vec![fake_device], 1.0);
    verifier.should_succeed = false; // Challenge-response fails

    // When the challenge-response verification runs
    let challenge = [0u8; 16];
    let emit_result = verifier.emit_challenge(&challenge);

    // Then the relay attack should be detected (device error)
    assert!(emit_result.is_err());
}

/// Tests RSSI to distance conversion accuracy
#[test]
fn test_ble_rssi_to_distance_estimation() {
    // Test various RSSI values and expected distance ranges
    // RSSI around -40 to -50 dBm typically indicates < 1 meter
    // RSSI around -60 to -70 dBm typically indicates 1-3 meters
    // RSSI around -80 to -90 dBm typically indicates > 3 meters

    let close_device = BLEDevice::new("close", -45);
    let medium_device = BLEDevice::new("medium", -65);
    let far_device = BLEDevice::new("far", -85);

    let close_verifier = MockBLEVerifier::new(vec![close_device], 0.5);
    let medium_verifier = MockBLEVerifier::new(vec![medium_device], 2.0);
    let far_verifier = MockBLEVerifier::new(vec![far_device], 5.0);

    // Distance estimates should match simulated distances
    let close_dist = close_verifier
        .estimate_distance(&close_verifier.devices[0])
        .unwrap();
    let medium_dist = medium_verifier
        .estimate_distance(&medium_verifier.devices[0])
        .unwrap();
    let far_dist = far_verifier
        .estimate_distance(&far_verifier.devices[0])
        .unwrap();

    assert!(close_dist < 1.0, "Close device should be < 1m");
    assert!(
        medium_dist >= 1.0 && medium_dist <= 3.0,
        "Medium should be 1-3m"
    );
    assert!(far_dist > 3.0, "Far device should be > 3m");
}

/// Tests BLE discovery timeout behavior
#[test]
fn test_ble_discovery_with_no_devices() {
    // When no WebBook devices are nearby
    let verifier = MockBLEVerifier::new(vec![], 0.0);

    // Discovery should return empty list (not error)
    let discovered = verifier.discover_nearby(Duration::from_secs(5)).unwrap();
    assert!(discovered.is_empty());
}

/// Tests BLE discovery failure handling
#[test]
fn test_ble_discovery_failure() {
    // When BLE hardware fails
    let verifier = MockBLEVerifier::failure();

    // Discovery should return error
    let result = verifier.discover_nearby(Duration::from_secs(5));
    assert!(result.is_err());
}

// =============================================================================
// NFC Exchange Tests (from contact_exchange.feature @nfc scenarios)
// =============================================================================

use webbook_core::exchange::{ManualConfirmationVerifier, ProximityVerifier};

/// Feature: Contact Card Exchange
/// Scenario: NFC contact exchange
/// Tests NFC exchange initiates when devices tap together
#[test]
fn test_nfc_exchange_initiation() {
    // Given Alice and Bob have NFC-capable devices
    // And both have NFC enabled
    // NFC is essentially manual confirmation with physical tap

    let verifier = ManualConfirmationVerifier::pre_confirmed();

    // When Alice and Bob tap their devices together
    // The proximity is verified by the physical tap itself
    let challenge = [1u8; 16];
    let emit_result = verifier.emit_challenge(&challenge);
    assert!(emit_result.is_ok());

    // And public keys should be exchanged (via NFC data transfer)
    let response_result = verifier.listen_for_response(Duration::from_secs(5));
    assert!(response_result.is_ok());
}

/// Feature: Contact Card Exchange
/// Scenario: NFC exchange timeout
/// Tests NFC mode times out after 30 seconds without contact
#[test]
fn test_nfc_exchange_timeout() {
    // Given Alice has initiated NFC mode
    let verifier = ManualConfirmationVerifier::with_state(false); // No confirmation

    // When 30 seconds pass without NFC contact
    let result = verifier.listen_for_response(Duration::from_secs(1));

    // Then NFC mode should timeout (no response without confirmation)
    assert!(matches!(result, Err(ProximityError::NoResponse)));
}

/// Tests NFC requires manual confirmation when device lacks NFC hardware
#[test]
fn test_nfc_fallback_to_manual_confirmation() {
    // When a device doesn't support NFC, manual confirmation is the fallback
    let verifier = ManualConfirmationVerifier::pre_confirmed();

    let challenge = [2u8; 16];
    assert!(verifier.emit_challenge(&challenge).is_ok());

    // Manual confirmation succeeds when user confirms
    let response = verifier
        .listen_for_response(Duration::from_secs(5))
        .unwrap();
    assert!(!response.is_empty());
}
