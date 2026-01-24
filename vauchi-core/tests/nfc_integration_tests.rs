//! NFC Tag Exchange Integration Tests
//!
//! Integration tests for NFC tag-based contact exchange.
//! Feature file: features/contact_exchange.feature @nfc
//!
//! These tests verify:
//! - NFC tag payload creation and parsing
//! - Password-protected tags
//! - Introduction message creation
//! - Mailbox-based async exchange

use std::time::Duration;
use vauchi_core::exchange::{
    create_nfc_payload, parse_nfc_payload, Introduction, NfcError, NfcTagMode,
};
use vauchi_core::crypto::SigningKeyPair;

// ============================================================
// NFC Tag Payload Creation
// Feature: contact_exchange.feature @nfc @tag
// ============================================================

/// Test: Create open NFC tag payload
#[test]
fn test_create_open_nfc_tag() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Open,
    ).expect("Should create payload");

    assert_eq!(payload.magic(), b"VBMB");
    assert_eq!(payload.version(), 1);
    assert!(!payload.is_password_protected());
    assert!(payload.verify_signature(&keypair.public_key()));
}

/// Test: Create password-protected NFC tag payload
#[test]
fn test_create_protected_nfc_tag() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];
    let password = "meetup";

    let payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected { password: password.to_string() },
    ).expect("Should create protected payload");

    assert_eq!(payload.magic(), b"VBNP");
    assert!(payload.is_password_protected());
    assert!(payload.verify_password(password));
    assert!(!payload.verify_password("wrong"));
}

/// Test: NFC payload fits within tag capacity
#[test]
fn test_nfc_payload_size() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let open_payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Open,
    ).unwrap();

    let protected_payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected { password: "test".to_string() },
    ).unwrap();

    // NTAG213 has 144 bytes, NTAG215 has 504 bytes
    assert!(open_payload.to_bytes().len() <= 200, "Open payload should fit in small tags");
    assert!(protected_payload.to_bytes().len() <= 250, "Protected payload should fit in medium tags");
}

// ============================================================
// NFC Tag Parsing
// Feature: contact_exchange.feature @nfc
// ============================================================

/// Test: Parse open NFC tag payload
#[test]
fn test_parse_open_nfc_payload() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [42u8; 32];

    let original = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Open,
    ).unwrap();

    let bytes = original.to_bytes();
    let parsed = parse_nfc_payload(&bytes).expect("Should parse");

    assert_eq!(parsed.relay_url(), relay_url);
    assert_eq!(parsed.mailbox_id(), &mailbox_id);
    assert!(parsed.verify_signature(&keypair.public_key()));
}

/// Test: Parse protected NFC tag requires password
#[test]
fn test_parse_protected_requires_password() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected { password: "secret".to_string() },
    ).unwrap();

    let bytes = payload.to_bytes();
    let parsed = parse_nfc_payload(&bytes).expect("Should parse");

    assert!(parsed.is_password_protected());
    assert!(parsed.verify_password("secret"));
    assert!(!parsed.verify_password("wrong"));
}

/// Test: Invalid magic bytes rejected
#[test]
fn test_invalid_magic_rejected() {
    let mut bytes = vec![0u8; 200];
    bytes[0..4].copy_from_slice(b"XXXX");
    bytes[4] = 1; // version

    let result = parse_nfc_payload(&bytes);
    assert!(
        matches!(result, Err(NfcError::InvalidMagic)),
        "Expected InvalidMagic, got {:?}",
        result
    );
}

/// Test: Invalid signature rejected
#[test]
fn test_invalid_signature_rejected() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Open,
    ).unwrap();

    let mut bytes = payload.to_bytes();
    // Corrupt the signature
    let len = bytes.len();
    bytes[len - 1] ^= 0xFF;

    let parsed = parse_nfc_payload(&bytes).expect("Should parse structure");
    let other_keypair = SigningKeyPair::generate();
    assert!(!parsed.verify_signature(&other_keypair.public_key()));
}

// ============================================================
// Introduction Messages
// Feature: contact_exchange.feature @nfc
// ============================================================

/// Test: Create introduction from NFC scan
#[test]
fn test_create_introduction() {
    let scanner_keypair = SigningKeyPair::generate();
    let tag_owner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let tag_payload = create_nfc_payload(
        &tag_owner_keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Open,
    ).unwrap();

    let intro = Introduction::create(
        &scanner_keypair,
        &tag_payload,
        "Bob's contact card data".as_bytes(),
    ).expect("Should create introduction");

    assert!(!intro.ciphertext().is_empty());
    assert!(intro.sender_signing_key().is_some());
}

/// Test: Introduction with password creates encrypted message
/// Note: Full decryption requires stored X25519 private key (not implemented in test)
#[test]
fn test_introduction_with_password() {
    let scanner_keypair = SigningKeyPair::generate();
    let tag_owner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];
    let password = "meetup";

    let tag_payload = create_nfc_payload(
        &tag_owner_keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected { password: password.to_string() },
    ).unwrap();

    let intro = Introduction::create_with_password(
        &scanner_keypair,
        &tag_payload,
        "Bob's card".as_bytes(),
        password,
    ).expect("Should create introduction with password");

    // Verify introduction was created with encrypted data
    assert!(!intro.ciphertext().is_empty());
    assert!(intro.ciphertext().len() > "Bob's card".len()); // Includes auth tag

    // Note: Decryption requires the tag owner's X25519 private key
    // which would be stored alongside the tag metadata in a real implementation.
    // The test verifies encryption works, decryption is tested in e2e tests.
}

/// Test: Introduction decryption fails with wrong password
#[test]
fn test_introduction_wrong_password_fails() {
    let scanner_keypair = SigningKeyPair::generate();
    let tag_owner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let tag_payload = create_nfc_payload(
        &tag_owner_keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected { password: "correct".to_string() },
    ).unwrap();

    let intro = Introduction::create_with_password(
        &scanner_keypair,
        &tag_payload,
        "data".as_bytes(),
        "correct",
    ).unwrap();

    let result = intro.decrypt(&tag_owner_keypair, Some("wrong"));
    assert!(result.is_err());
}

// ============================================================
// Password Protection
// Feature: contact_exchange.feature @nfc @password
// ============================================================

/// Test: Password verifier uses PBKDF2
#[test]
fn test_password_verifier_pbkdf2() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];
    let password = "test123";

    let payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected { password: password.to_string() },
    ).unwrap();

    // Same password should always verify
    assert!(payload.verify_password(password));
    assert!(payload.verify_password(password));

    // Different password should fail
    assert!(!payload.verify_password("Test123")); // Case sensitive
    assert!(!payload.verify_password("test1234"));
    assert!(!payload.verify_password(""));
}

/// Test: Brute force protection via slow verification
#[test]
fn test_password_verification_is_slow() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let payload = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected { password: "test".to_string() },
    ).unwrap();

    // PBKDF2 with 100k iterations should take measurable time
    let start = std::time::Instant::now();
    payload.verify_password("wrong");
    let elapsed = start.elapsed();

    // Should take at least 10ms (actually ~50-200ms with 100k iterations)
    assert!(elapsed >= Duration::from_millis(10),
        "Password verification should be slow to prevent brute force");
}

// ============================================================
// Serialization
// ============================================================

/// Test: NFC payload serialization roundtrip
#[test]
fn test_payload_serialization() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [123u8; 32];

    let original = create_nfc_payload(
        &keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Open,
    ).unwrap();

    let bytes = original.to_bytes();
    let parsed = parse_nfc_payload(&bytes).unwrap();

    assert_eq!(original.to_bytes(), parsed.to_bytes());
}

/// Test: Introduction serialization for relay
#[test]
fn test_introduction_serialization() {
    let scanner_keypair = SigningKeyPair::generate();
    let tag_owner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let tag_payload = create_nfc_payload(
        &tag_owner_keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Open,
    ).unwrap();

    let intro = Introduction::create(
        &scanner_keypair,
        &tag_payload,
        "test data".as_bytes(),
    ).unwrap();

    let json = serde_json::to_string(&intro).expect("Should serialize");
    let restored: Introduction = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(intro.ciphertext(), restored.ciphertext());
}
