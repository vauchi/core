// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TDD Tests for Contact Exchange Protocol
//!
//! These tests are written FIRST (RED phase) before implementation.

use vauchi_core::Identity;
use vauchi_core::exchange::{ExchangeQR, X3DH, X3DHKeyPair};

// =============================================================================
// X3DH Key Agreement Tests
// =============================================================================

/// Tests that X3DH key agreement produces the same shared secret on both sides
// @scenario: contact_exchange :: X3DH key agreement during exchange
// @scenario: security :: Shared key derivation via X3DH
// @internal
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
// @scenario: contact_exchange :: X3DH key agreement during exchange
// @internal
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
// @scenario: contact_exchange :: Mutual QR uses fresh ephemeral keys for forward secrecy
// @internal
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
// @scenario: contact_exchange :: Exchange creates mutual keys
// @scenario: security :: Contact cards are encrypted in transit
// @internal
#[test]
fn test_x3dh_shared_secret_usable_for_encryption() {
    use vauchi_core::crypto::{decrypt, encrypt};

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
// @scenario: contact_exchange :: Generate exchange QR code
// @internal
#[test]
fn test_generate_qr_contains_public_key() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(&identity, &ephemeral);

    assert_eq!(qr.public_key(), identity.signing_public_key());
}

/// Tests QR code roundtrip encode/decode
// @scenario: contact_exchange :: Generate exchange QR code
// @internal
#[test]
fn test_qr_roundtrip_encode_decode() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let original = ExchangeQR::generate(&identity, &ephemeral);

    let encoded = original.to_data_string();
    let decoded = ExchangeQR::from_data_string(&encoded).expect("Decoding should succeed");

    assert_eq!(original.public_key(), decoded.public_key());
    assert_eq!(original.exchange_token(), decoded.exchange_token());
}

/// Tests that QR code expires after 5 minutes
// @scenario: contact_exchange :: QR code expiration
// @internal
#[test]
fn test_qr_expires_after_5_minutes() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(&identity, &ephemeral);

    // Fresh QR should not be expired
    assert!(!qr.is_expired());

    // Create a QR with timestamp 6 minutes in the past
    let old_qr = ExchangeQR::generate_with_timestamp(
        &identity,
        &ephemeral,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 360, // 6 minutes ago
    );

    assert!(old_qr.is_expired());
}

/// Tests QR signature verification
// @scenario: contact_exchange :: Exchange verifies identity
// @internal
#[test]
fn test_qr_signature_verification() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(&identity, &ephemeral);

    assert!(qr.verify_signature());
}

/// Tests that malformed QR data is rejected
// @scenario: contact_exchange :: Handle malformed QR code
// @internal
#[test]
fn test_malformed_qr_rejected() {
    let result = ExchangeQR::from_data_string("not-valid-qr-data");
    result.expect_err("expected error");

    let result = ExchangeQR::from_data_string("");
    result.expect_err("expected error");
}

/// Tests that QR from different app/protocol is rejected
// @scenario: contact_exchange :: Handle non-Vauchi QR code
// @internal
#[test]
fn test_non_vauchi_qr_rejected() {
    // Random base64 data that's not our protocol
    let fake_qr = "eyJub3QiOiJ3ZWJib29rIn0=";
    let result = ExchangeQR::from_data_string(fake_qr);
    result.expect_err("expected error");
}

// =============================================================================
// BLE Proximity Tests (from contact_exchange.feature @ble scenarios)
// =============================================================================

use std::time::Duration;
use vauchi_core::exchange::{BLEDevice, BLEProximityVerifier, MockBLEVerifier, ProximityError};

/// Feature: Contact Card Exchange
/// Scenario: Discover nearby Vauchi users via BLE
/// Tests that BLE can discover nearby devices advertising Vauchi
// @scenario: contact_exchange :: Discover nearby Vauchi users via BLE
// @internal
#[test]
fn test_ble_discover_nearby_vauchi_users() {
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
// @scenario: contact_exchange :: Initiate BLE exchange
// @internal
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
    result.expect("expected success");
}

/// Feature: Contact Card Exchange
/// Scenario: BLE exchange blocked when too far
/// Tests that exchange is blocked when devices are more than 2 meters apart
// @scenario: contact_exchange :: BLE exchange blocked when too far
// @internal
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
// @scenario: contact_exchange :: BLE exchange with relay attack prevention
// @scenario: security :: Relay attack prevention on BLE
// @internal
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
    emit_result.expect_err("expected error");
}

/// Tests RSSI to distance conversion accuracy
// @internal
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
    assert!((1.0..=3.0).contains(&medium_dist), "Medium should be 1-3m");
    assert!(far_dist > 3.0, "Far device should be > 3m");
}

/// Tests BLE discovery timeout behavior
// @internal
#[test]
fn test_ble_discovery_with_no_devices() {
    // When no Vauchi devices are nearby
    let verifier = MockBLEVerifier::new(vec![], 0.0);

    // Discovery should return empty list (not error)
    let discovered = verifier.discover_nearby(Duration::from_secs(5)).unwrap();
    assert!(discovered.is_empty());
}

/// Tests BLE discovery failure handling
// @internal
#[test]
fn test_ble_discovery_failure() {
    // When BLE hardware fails
    let verifier = MockBLEVerifier::failure();

    // Discovery should return error
    let result = verifier.discover_nearby(Duration::from_secs(5));
    result.expect_err("expected error");
}

// =============================================================================
// Manual Proximity Exchange Tests (manual confirmation verifier)
// =============================================================================

use vauchi_core::exchange::{ManualConfirmationVerifier, ProximityVerifier};

/// Tests manual confirmation exchange initiates when both parties confirm
// @scenario: contact_exchange :: Desktop exchange without audio (requires confirmation)
// @internal
#[test]
fn test_manual_proximity_exchange_initiation() {
    // Given Alice and Bob are physically present
    // And both confirm the exchange manually

    let verifier = ManualConfirmationVerifier::pre_confirmed();

    // When both parties confirm proximity
    let challenge = [1u8; 16];
    let emit_result = verifier.emit_challenge(&challenge);
    emit_result.expect("expected success");

    // And public keys should be exchanged
    let response_result = verifier.listen_for_response(Duration::from_secs(5));
    response_result.expect("expected success");
}

/// Tests manual confirmation exchange times out without confirmation
// @internal
#[test]
fn test_manual_proximity_exchange_timeout() {
    // Given Alice has initiated manual confirmation mode
    let verifier = ManualConfirmationVerifier::with_state(false); // No confirmation

    // When no confirmation is received within the timeout
    let result = verifier.listen_for_response(Duration::from_secs(1));

    // Then the exchange should timeout (no response without confirmation)
    assert!(matches!(result, Err(ProximityError::NoResponse)));
}

/// Tests manual confirmation succeeds when user confirms
// @internal
#[test]
fn test_manual_confirmation_verifier() {
    // Manual confirmation is the fallback when hardware proximity isn't available
    let verifier = ManualConfirmationVerifier::pre_confirmed();

    let challenge = [2u8; 16];
    verifier
        .emit_challenge(&challenge)
        .expect("expected success");

    // Manual confirmation succeeds when user confirms
    let response = verifier
        .listen_for_response(Duration::from_secs(5))
        .unwrap();
    assert!(!response.is_empty());
}

// =============================================================================
// Encrypted Exchange Message Tests (Critical Security Fix)
// Reference: features/contact_exchange.feature - exchange messages must be encrypted
// =============================================================================

use vauchi_core::exchange::EncryptedExchangeMessage;

/// Tests that exchange messages are properly encrypted with X3DH shared secret.
/// This ensures the relay cannot see identity keys or display names.
// @scenario: security :: Contact cards are encrypted in transit
// @scenario: contact_exchange :: X3DH key agreement during exchange
// @internal
#[test]
fn test_exchange_message_is_encrypted_not_plaintext() {
    // Given Alice and Bob want to exchange contacts
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    // When Alice creates an encrypted exchange message
    let alice_identity_key = [0x41u8; 32]; // Alice's signing key
    let alice_display_name = "Alice Smith";

    let (encrypted_msg, _shared_secret) = EncryptedExchangeMessage::create(
        &alice,
        bob.public_key(),
        &alice_identity_key,
        alice_display_name,
    )
    .expect("Creating encrypted exchange message should succeed");

    // Then the ciphertext should NOT contain the plaintext identity key or name
    let ciphertext_str = String::from_utf8_lossy(&encrypted_msg.ciphertext);
    assert!(
        !ciphertext_str.contains("Alice Smith"),
        "Display name must not appear in plaintext"
    );
    assert!(
        !encrypted_msg
            .ciphertext
            .windows(32)
            .any(|w| w == alice_identity_key),
        "Identity key must not appear in plaintext"
    );

    // And the ephemeral public key should be included (needed for X3DH)
    assert_ne!(
        encrypted_msg.ephemeral_public_key, [0u8; 32],
        "Ephemeral key must be present"
    );
}

/// Tests that the recipient can decrypt the exchange message using X3DH.
// @scenario: contact_exchange :: Exchange creates mutual keys
// @internal
#[test]
fn test_exchange_message_recipient_can_decrypt() {
    // Given Alice creates an encrypted exchange message for Bob
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let alice_identity_key = [0x42u8; 32];
    let alice_display_name = "Alice Johnson";

    let (encrypted_msg, _alice_secret) = EncryptedExchangeMessage::create(
        &alice,
        bob.public_key(),
        &alice_identity_key,
        alice_display_name,
    )
    .expect("Creating message should succeed");

    // When Bob receives and decrypts the message
    let (payload, _shared_secret) = encrypted_msg
        .decrypt(&bob)
        .expect("Bob should be able to decrypt");

    // Then Bob should recover Alice's identity key, exchange key, and name
    assert_eq!(payload.identity_key, alice_identity_key);
    assert_eq!(payload.exchange_key, *alice.public_key());
    assert_eq!(payload.display_name, alice_display_name);
}

/// Tests that wrong keys cannot decrypt the exchange message.
// @scenario: security :: Man-in-the-middle detection during exchange
// @internal
#[test]
fn test_exchange_message_wrong_key_fails_decrypt() {
    // Given Alice creates an encrypted exchange message for Bob
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();
    let charlie = X3DHKeyPair::generate(); // Attacker

    let alice_identity_key = [0x43u8; 32];
    let alice_display_name = "Alice";

    let (encrypted_msg, _) = EncryptedExchangeMessage::create(
        &alice,
        bob.public_key(),
        &alice_identity_key,
        alice_display_name,
    )
    .expect("Creating message should succeed");

    // When Charlie (attacker) tries to decrypt
    let result = encrypted_msg.decrypt(&charlie);

    // Then decryption should fail
    assert!(result.is_err(), "Wrong key should fail to decrypt");
}

/// Tests that the relay cannot read exchange message contents.
/// This is the critical security property - relay only sees opaque ciphertext.
// @scenario: security :: Server cannot access plaintext
// @scenario: relay_network :: Relay only sees encrypted blobs
// @scenario: relay_network :: Relay cannot read offloaded blobs
// @internal
#[test]
fn test_relay_cannot_read_exchange_message() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let sensitive_name = "John Doe - CEO of SecretCorp";
    let identity_key = [0x44u8; 32];

    let (encrypted_msg, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &identity_key, sensitive_name)
            .expect("Creating message should succeed");

    // The relay only sees:
    // 1. Ephemeral public key (random, unlinkable to identity)
    // 2. Ciphertext (opaque bytes)

    // Verify no sensitive data leaks in the wire format
    let wire_bytes = encrypted_msg.to_bytes().unwrap();
    let wire_str = String::from_utf8_lossy(&wire_bytes);

    assert!(
        !wire_str.contains("John Doe"),
        "Name must not leak to relay"
    );
    assert!(
        !wire_str.contains("SecretCorp"),
        "Name must not leak to relay"
    );
    assert!(
        !wire_bytes.windows(32).any(|w| w == identity_key),
        "Identity key must not leak to relay"
    );
}

/// Tests serialization roundtrip for encrypted exchange messages.
// @internal
#[test]
fn test_encrypted_exchange_message_roundtrip() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (original, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x45u8; 32], "Test User")
            .expect("Creating message should succeed");

    // Serialize and deserialize
    let bytes = original.to_bytes().expect("serialization should succeed");
    let restored =
        EncryptedExchangeMessage::from_bytes(&bytes).expect("Deserialization should succeed");

    assert_eq!(restored.ephemeral_public_key, original.ephemeral_public_key);
    assert_eq!(restored.ciphertext, original.ciphertext);
}
