// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let (alice_secret, alice_ephemeral_public) =
        X3DH::initiate(&alice_keys, bob_keys.public_key()).expect("Key agreement should succeed");

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

    let (alice_bob_secret, _alice_ephemeral) = X3DH::initiate(&alice, bob.public_key()).unwrap();

    let (alice_charlie_secret, _) = X3DH::initiate(&alice, charlie.public_key()).unwrap();

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

    let message = b"Hello Bob!";
    let ciphertext = encrypt(&alice_secret, message).expect("Encryption should succeed");

    let decrypted = decrypt(&bob_secret, &ciphertext).expect("Decryption should succeed");
    assert_eq!(decrypted, message);
}

// =============================================================================
// =============================================================================

/// Tests that QR code contains public key
// @scenario: contact_exchange :: Generate exchange QR code
// @internal
#[test]
fn test_generate_qr_contains_public_key() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    assert_eq!(qr.public_key(), identity.signing_public_key());
}

/// Tests QR code roundtrip encode/decode
// @scenario: contact_exchange :: Generate exchange QR code
// @internal
#[test]
fn test_qr_roundtrip_encode_decode() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let original = ExchangeQR::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

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
    let qr = ExchangeQR::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    assert!(!qr.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()));

    let old_qr = ExchangeQR::generate_with_timestamp(
        &identity,
        &ephemeral,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 360, // 6 minutes ago
    );

    assert!(old_qr.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()));
}

/// Tests QR signature verification
// @scenario: contact_exchange :: Exchange verifies identity
// @internal
#[test]
fn test_qr_signature_verification() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

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
// Manual Proximity Exchange Tests (manual confirmation verifier)
// =============================================================================

use std::time::Duration;
use vauchi_core::exchange::{ManualConfirmationVerifier, ProximityError, ProximityVerifier};

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

    let bytes = original.to_bytes().expect("serialization should succeed");
    let restored =
        EncryptedExchangeMessage::from_bytes(&bytes).expect("Deserialization should succeed");

    assert_eq!(restored.ephemeral_public_key, original.ephemeral_public_key);
    assert_eq!(restored.ciphertext, original.ciphertext);
}

// @internal
#[test]
fn a_ble_payload_keeps_the_time_it_was_generated_at() {
    let alice = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    let payload = vauchi_core::exchange::ExchangeBle::generate(&alice, &ephemeral, 1_234_567);

    assert_eq!(payload.timestamp(), 1_234_567);
}

// @internal
#[test]
fn an_exchange_id_debugs_as_its_hex() {
    let id = vauchi_core::exchange::ExchangeId::from_bytes([0xab; 32]);

    assert_eq!(
        format!("{id:?}"),
        format!("ExchangeId({})", "ab".repeat(32))
    );
}
