//! NFC Roundtrip Tests
//!
//! Tests for full NFC exchange roundtrip:
//! Tag creation -> Introduction creation -> Introduction decryption
//!
//! Feature file: features/contact_exchange.feature @nfc @e2e

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::exchange::{create_nfc_tag, Introduction, NfcTagMode};

// ============================================================
// Full Roundtrip Tests
// Feature: contact_exchange.feature @nfc @e2e
// ============================================================

/// Test: Open tag full roundtrip - create tag, create intro, decrypt intro
#[test]
fn test_open_tag_full_roundtrip() {
    let tag_owner_keypair = SigningKeyPair::generate();
    let scanner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [42u8; 32];
    let contact_data = b"Scanner's contact card data";

    // 1. Tag owner creates NFC tag (returns payload + private key for storage)
    let tag_result = create_nfc_tag(&tag_owner_keypair, relay_url, &mailbox_id, NfcTagMode::Open)
        .expect("Should create tag");

    // Verify payload is valid
    assert!(!tag_result.payload().is_password_protected());
    assert_eq!(tag_result.payload().relay_url(), relay_url);

    // 2. Scanner creates introduction encrypted to tag's exchange key
    let intro = Introduction::create(&scanner_keypair, tag_result.payload(), contact_data)
        .expect("Should create introduction");

    assert!(!intro.ciphertext().is_empty());

    // 3. Tag owner decrypts introduction using stored exchange keypair
    let decrypted = intro
        .decrypt_with_exchange_key(tag_result.exchange_keypair(), None)
        .expect("Should decrypt introduction");

    // Verify decryption produces original data
    assert_eq!(decrypted, contact_data);
}

/// Test: Protected tag full roundtrip with password
#[test]
fn test_protected_tag_full_roundtrip() {
    let tag_owner_keypair = SigningKeyPair::generate();
    let scanner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];
    let password = "meetup2024";
    let contact_data = b"Bob's full contact card with email and phone";

    // 1. Tag owner creates password-protected NFC tag
    let tag_result = create_nfc_tag(
        &tag_owner_keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected {
            password: password.to_string(),
        },
    )
    .expect("Should create protected tag");

    // Verify password protection
    assert!(tag_result.payload().is_password_protected());
    assert!(tag_result.payload().verify_password(password));

    // 2. Scanner creates introduction with correct password
    let intro = Introduction::create_with_password(
        &scanner_keypair,
        tag_result.payload(),
        contact_data,
        password,
    )
    .expect("Should create introduction");

    // 3. Tag owner decrypts with stored key, password, and salt from payload
    let salt = tag_result
        .payload()
        .password_salt()
        .expect("Protected tag should have salt");
    let decrypted = intro
        .decrypt_with_exchange_key(tag_result.exchange_keypair(), Some((password, salt)))
        .expect("Should decrypt introduction");

    assert_eq!(decrypted, contact_data);
}

/// Test: Wrong password fails decryption
#[test]
fn test_wrong_password_fails_decryption() {
    let tag_owner_keypair = SigningKeyPair::generate();
    let scanner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];
    let password = "correct";
    let wrong_password = "wrong";

    // Create protected tag
    let tag_result = create_nfc_tag(
        &tag_owner_keypair,
        relay_url,
        &mailbox_id,
        NfcTagMode::Protected {
            password: password.to_string(),
        },
    )
    .unwrap();

    // Create introduction with correct password
    let intro = Introduction::create_with_password(
        &scanner_keypair,
        tag_result.payload(),
        b"secret data",
        password,
    )
    .unwrap();

    // Decryption with wrong password should fail
    let salt = tag_result.payload().password_salt().unwrap();
    let result = intro
        .decrypt_with_exchange_key(tag_result.exchange_keypair(), Some((wrong_password, salt)));
    assert!(result.is_err(), "Wrong password should fail decryption");
}

/// Test: Wrong exchange key fails decryption
#[test]
fn test_wrong_exchange_key_fails_decryption() {
    let tag_owner_keypair = SigningKeyPair::generate();
    let scanner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    // Create tag
    let tag_result =
        create_nfc_tag(&tag_owner_keypair, relay_url, &mailbox_id, NfcTagMode::Open).unwrap();

    // Create introduction
    let intro = Introduction::create(&scanner_keypair, tag_result.payload(), b"data").unwrap();

    // Try to decrypt with a DIFFERENT exchange keypair
    let wrong_keypair = vauchi_core::exchange::X3DHKeyPair::generate();
    let result = intro.decrypt_with_exchange_key(&wrong_keypair, None);

    // Should fail because key agreement produces wrong shared secret
    assert!(result.is_err(), "Wrong exchange key should fail decryption");
}

/// Test: Tag creation result contains valid exchange keypair
#[test]
fn test_tag_result_contains_exchange_keypair() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let tag_result = create_nfc_tag(&keypair, relay_url, &mailbox_id, NfcTagMode::Open).unwrap();

    // Exchange keypair public key should match payload's exchange key
    assert_eq!(
        tag_result.exchange_keypair().public_key(),
        tag_result.payload().exchange_key()
    );
}

/// Test: Exchange keypair can be serialized for storage
#[test]
fn test_exchange_keypair_serializable() {
    let keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    let tag_result = create_nfc_tag(&keypair, relay_url, &mailbox_id, NfcTagMode::Open).unwrap();

    // Get the secret bytes for storage
    let secret_bytes = tag_result.exchange_keypair().secret_bytes();

    // Should be able to restore keypair from bytes
    let restored = vauchi_core::exchange::X3DHKeyPair::from_bytes(secret_bytes);
    assert_eq!(
        restored.public_key(),
        tag_result.exchange_keypair().public_key()
    );
}

/// Test: Large contact data roundtrip
#[test]
fn test_large_contact_data_roundtrip() {
    let tag_owner_keypair = SigningKeyPair::generate();
    let scanner_keypair = SigningKeyPair::generate();
    let relay_url = "wss://relay.vauchi.app";
    let mailbox_id = [0u8; 32];

    // Large contact data (simulating full contact card with photo)
    let large_data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();

    let tag_result =
        create_nfc_tag(&tag_owner_keypair, relay_url, &mailbox_id, NfcTagMode::Open).unwrap();

    let intro = Introduction::create(&scanner_keypair, tag_result.payload(), &large_data).unwrap();

    let decrypted = intro
        .decrypt_with_exchange_key(tag_result.exchange_keypair(), None)
        .unwrap();

    assert_eq!(decrypted, large_data);
}
