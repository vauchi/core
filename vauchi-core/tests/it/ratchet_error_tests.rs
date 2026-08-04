// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ratchet Error Tests
//!
//! Tests for Double Ratchet error conditions and edge cases.
//! These tests ensure the cryptographic protocol handles errors correctly.

use vauchi_core::{SymmetricKey, crypto::ratchet::DoubleRatchetState, exchange::X3DHKeyPair};

// =============================================================================
// =============================================================================

/// Test: Messages can be decrypted in order
// @internal
#[test]
fn test_ratchet_messages_in_order() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let msg1 = b"Message 1";
    let msg2 = b"Message 2";
    let msg3 = b"Message 3";

    let enc1 = alice_ratchet.encrypt(msg1).unwrap();
    let enc2 = alice_ratchet.encrypt(msg2).unwrap();
    let enc3 = alice_ratchet.encrypt(msg3).unwrap();

    let dec1 = bob_ratchet.decrypt(&enc1).unwrap();
    let dec2 = bob_ratchet.decrypt(&enc2).unwrap();
    let dec3 = bob_ratchet.decrypt(&enc3).unwrap();

    assert_eq!(dec1, msg1);
    assert_eq!(dec2, msg2);
    assert_eq!(dec3, msg3);
}

/// Test: Out-of-order messages can be handled (with message skipping)
// @internal
#[test]
fn test_ratchet_handles_message_skip() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let msg1 = b"Message 1";
    let msg2 = b"Message 2";
    let msg3 = b"Message 3";

    let enc1 = alice_ratchet.encrypt(msg1).unwrap();
    let _enc2 = alice_ratchet.encrypt(msg2).unwrap(); // Skip this one
    let enc3 = alice_ratchet.encrypt(msg3).unwrap();

    let dec1 = bob_ratchet.decrypt(&enc1).unwrap();
    assert_eq!(dec1, msg1);

    // Bob receives message 3 (skipping 2)
    // This tests the ratchet's ability to handle skipped messages
    let decrypted = bob_ratchet
        .decrypt(&enc3)
        .expect("message within the supported skip window must decrypt");
    assert_eq!(decrypted, msg3);
}

/// Test: Duplicate message detection
// @scenario: security :: Replay attack prevention
// @internal
#[test]
fn test_ratchet_rejects_duplicate_message() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let plaintext = b"Hello Bob";
    let encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, plaintext);

    // Bob tries to decrypt the same message again (replay attack)
    let replay_result = bob_ratchet.decrypt(&encrypted);

    assert!(
        replay_result.is_err(),
        "Duplicate message should be rejected to prevent replay attacks"
    );
}

// =============================================================================
// =============================================================================

/// Test: Ratchet detects corrupted DH public key
// @internal
#[test]
fn test_ratchet_fails_on_corrupted_dh_key() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let plaintext = b"Secret message";
    let mut encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    // Corrupt the DH public key
    encrypted.dh_public[0] ^= 0xFF;

    let result = bob_ratchet.decrypt(&encrypted);
    assert!(result.is_err(), "Corrupted DH key should fail decryption");
}

/// Test: Ratchet handles empty plaintext
// @internal
#[test]
fn test_ratchet_handles_empty_plaintext() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let plaintext = b"";
    let encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, plaintext);
}

/// Test: Ratchet handles large plaintext
// @internal
#[test]
fn test_ratchet_handles_large_plaintext() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Large message (1MB)
    let plaintext = vec![0xABu8; 1024 * 1024];
    let encrypted = alice_ratchet.encrypt(&plaintext).unwrap();

    let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, plaintext);
}

// =============================================================================
// =============================================================================

/// Test: Different shared secrets produce different ratchets
// @scenario: security :: Forward secrecy via Double Ratchet
// @internal
#[test]
fn test_different_secrets_produce_different_ratchets() {
    let secret1 = SymmetricKey::generate();
    let secret2 = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut ratchet1 =
        DoubleRatchetState::initialize_initiator(&secret1, *bob_dh.public_key()).unwrap();
    let mut ratchet2 =
        DoubleRatchetState::initialize_initiator(&secret2, *bob_dh.public_key()).unwrap();

    let plaintext = b"Same message";
    let enc1 = ratchet1.encrypt(plaintext).unwrap();
    let enc2 = ratchet2.encrypt(plaintext).unwrap();

    assert_ne!(
        enc1.ciphertext, enc2.ciphertext,
        "Different secrets should produce different ciphertexts"
    );
}

/// Test: Same plaintext encrypts differently each time (nonce)
// @scenario: security :: Forward secrecy via Double Ratchet
// @internal
#[test]
fn test_same_plaintext_different_ciphertext() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    let plaintext = b"Same message";
    let enc1 = alice_ratchet.encrypt(plaintext).unwrap();
    let enc2 = alice_ratchet.encrypt(plaintext).unwrap();

    assert_ne!(
        enc1.ciphertext, enc2.ciphertext,
        "Same plaintext should encrypt differently each time (semantic security)"
    );
}

// =============================================================================
// =============================================================================

/// Test: Bidirectional ratchet conversation
// @scenario: security :: Forward secrecy via Double Ratchet
// @internal
#[test]
fn test_bidirectional_conversation() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let msg1 = b"Hello Bob";
    let enc1 = alice_ratchet.encrypt(msg1).unwrap();
    let dec1 = bob_ratchet.decrypt(&enc1).unwrap();
    assert_eq!(dec1, msg1);

    let msg2 = b"Hi Alice";
    let enc2 = bob_ratchet.encrypt(msg2).unwrap();
    let dec2 = alice_ratchet.decrypt(&enc2).unwrap();
    assert_eq!(dec2, msg2);

    let msg3 = b"How are you?";
    let enc3 = alice_ratchet.encrypt(msg3).unwrap();
    let dec3 = bob_ratchet.decrypt(&enc3).unwrap();
    assert_eq!(dec3, msg3);

    let msg4 = b"I'm fine!";
    let enc4 = bob_ratchet.encrypt(msg4).unwrap();
    let dec4 = alice_ratchet.decrypt(&enc4).unwrap();
    assert_eq!(dec4, msg4);
}

/// Test: Multiple consecutive messages from same party
// @internal
#[test]
fn test_consecutive_messages_same_party() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let messages: Vec<&[u8]> = vec![
        b"Message 1",
        b"Message 2",
        b"Message 3",
        b"Message 4",
        b"Message 5",
    ];

    let encrypted: Vec<_> = messages
        .iter()
        .map(|m| alice_ratchet.encrypt(m).unwrap())
        .collect();

    for (enc, expected) in encrypted.iter().zip(messages.iter()) {
        let decrypted = bob_ratchet.decrypt(enc).unwrap();
        assert_eq!(decrypted, *expected);
    }
}

// =============================================================================
// =============================================================================

/// Test: Ratchet state can be serialized and restored
// @internal
#[test]
fn test_ratchet_state_serialization() {
    use vauchi_core::{Contact, ContactCard, Vauchi};

    let mut alice_wb: Vauchi = Vauchi::in_memory().unwrap();
    let mut bob_wb: Vauchi = Vauchi::in_memory().unwrap();

    alice_wb.create_identity("Alice").unwrap();
    bob_wb.create_identity("Bob").unwrap();

    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    // Create contacts first (required for foreign key)
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();

    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone(), 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        shared_secret.clone(),
        0,
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    let alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_contact_id, &alice_ratchet, true)
        .unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, false)
        .unwrap();

    let (loaded_alice, is_initiator_a) = alice_wb
        .storage()
        .ratchets()
        .load_ratchet_state(&bob_contact_id)
        .unwrap()
        .unwrap();
    let (loaded_bob, is_initiator_b) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state(&alice_contact_id)
        .unwrap()
        .unwrap();

    assert!(is_initiator_a);
    assert!(!is_initiator_b);

    // Use loaded states for communication
    let mut alice = loaded_alice;
    let mut bob = loaded_bob;

    let plaintext = b"Test message after restore";
    let encrypted = alice.encrypt(plaintext).unwrap();
    let decrypted = bob.decrypt(&encrypted).unwrap();

    assert_eq!(decrypted, plaintext);
}

/// Test: Ratchet message serialization roundtrip
// @internal
#[test]
fn test_ratchet_message_serialization() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    let plaintext = b"Test message";
    let encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    let json = serde_json::to_string(&encrypted).unwrap();

    let restored: vauchi_core::crypto::ratchet::RatchetMessage =
        serde_json::from_str(&json).unwrap();

    assert_eq!(encrypted.dh_public, restored.dh_public);
    assert_eq!(encrypted.dh_generation, restored.dh_generation);
    assert_eq!(encrypted.message_index, restored.message_index);
    assert_eq!(
        encrypted.previous_chain_length,
        restored.previous_chain_length
    );
    assert_eq!(encrypted.ciphertext, restored.ciphertext);
}
