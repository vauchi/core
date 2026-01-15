//! TDD Tests for Contact Exchange Protocol
//!
//! These tests are written FIRST (RED phase) before implementation.

use webbook_core::exchange::{
    ExchangeQR, X3DH, X3DHKeyPair,
};
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
    let (alice_secret, alice_ephemeral_public) = X3DH::initiate(
        &alice_keys,
        bob_keys.public_key(),
    ).expect("Key agreement should succeed");

    // Bob responds using Alice's ephemeral public key
    let bob_secret = X3DH::respond(
        &bob_keys,
        alice_keys.public_key(),
        &alice_ephemeral_public,
    ).expect("Key agreement should succeed");

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
    let (alice_bob_secret, alice_ephemeral) = X3DH::initiate(&alice, bob.public_key()).unwrap();

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
    use webbook_core::crypto::{encrypt, decrypt};

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
            .as_secs() - 360, // 6 minutes ago
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
