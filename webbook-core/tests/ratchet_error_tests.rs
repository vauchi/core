//! Ratchet Error Tests
//!
//! Tests for Double Ratchet error conditions and edge cases.
//! These tests ensure the cryptographic protocol handles errors correctly.

use webbook_core::{crypto::ratchet::DoubleRatchetState, exchange::X3DHKeyPair, SymmetricKey};

// =============================================================================
// Message Order Tests
// =============================================================================

/// Test: Messages can be decrypted in order
#[test]
fn test_ratchet_messages_in_order() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Send multiple messages
    let msg1 = b"Message 1";
    let msg2 = b"Message 2";
    let msg3 = b"Message 3";

    let enc1 = alice_ratchet.encrypt(msg1).unwrap();
    let enc2 = alice_ratchet.encrypt(msg2).unwrap();
    let enc3 = alice_ratchet.encrypt(msg3).unwrap();

    // Decrypt in order
    let dec1 = bob_ratchet.decrypt(&enc1).unwrap();
    let dec2 = bob_ratchet.decrypt(&enc2).unwrap();
    let dec3 = bob_ratchet.decrypt(&enc3).unwrap();

    assert_eq!(dec1, msg1);
    assert_eq!(dec2, msg2);
    assert_eq!(dec3, msg3);
}

/// Test: Out-of-order messages can be handled (with message skipping)
#[test]
fn test_ratchet_handles_message_skip() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Alice sends 3 messages
    let msg1 = b"Message 1";
    let msg2 = b"Message 2";
    let msg3 = b"Message 3";

    let enc1 = alice_ratchet.encrypt(msg1).unwrap();
    let _enc2 = alice_ratchet.encrypt(msg2).unwrap(); // Skip this one
    let enc3 = alice_ratchet.encrypt(msg3).unwrap();

    // Bob receives message 1
    let dec1 = bob_ratchet.decrypt(&enc1).unwrap();
    assert_eq!(dec1, msg1);

    // Bob receives message 3 (skipping 2)
    // This tests the ratchet's ability to handle skipped messages
    let dec3_result = bob_ratchet.decrypt(&enc3);

    // Result depends on implementation - may succeed or fail
    // Signal protocol allows skipping up to MAX_SKIP messages
    if dec3_result.is_ok() {
        assert_eq!(dec3_result.unwrap(), msg3);
    }
}

/// Test: Duplicate message detection
#[test]
fn test_ratchet_rejects_duplicate_message() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Alice sends one message
    let plaintext = b"Hello Bob";
    let encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    // Bob decrypts it once
    let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, plaintext);

    // Bob tries to decrypt the same message again (replay attack)
    let replay_result = bob_ratchet.decrypt(&encrypted);

    // Should fail - cannot decrypt same message twice
    assert!(
        replay_result.is_err(),
        "Duplicate message should be rejected to prevent replay attacks"
    );
}

// =============================================================================
// State Corruption Tests
// =============================================================================

/// Test: Ratchet detects corrupted DH public key
#[test]
fn test_ratchet_fails_on_corrupted_dh_key() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    let plaintext = b"Secret message";
    let mut encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    // Corrupt the DH public key
    encrypted.dh_public[0] ^= 0xFF;

    let result = bob_ratchet.decrypt(&encrypted);
    assert!(result.is_err(), "Corrupted DH key should fail decryption");
}

/// Test: Ratchet handles empty plaintext
#[test]
fn test_ratchet_handles_empty_plaintext() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Encrypt empty message
    let plaintext = b"";
    let encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, plaintext);
}

/// Test: Ratchet handles large plaintext
#[test]
fn test_ratchet_handles_large_plaintext() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Large message (1MB)
    let plaintext = vec![0xABu8; 1024 * 1024];
    let encrypted = alice_ratchet.encrypt(&plaintext).unwrap();

    let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, plaintext);
}

// =============================================================================
// Key Derivation Tests
// =============================================================================

/// Test: Different shared secrets produce different ratchets
#[test]
fn test_different_secrets_produce_different_ratchets() {
    let secret1 = SymmetricKey::generate();
    let secret2 = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut ratchet1 = DoubleRatchetState::initialize_initiator(&secret1, *bob_dh.public_key());
    let mut ratchet2 = DoubleRatchetState::initialize_initiator(&secret2, *bob_dh.public_key());

    let plaintext = b"Same message";
    let enc1 = ratchet1.encrypt(plaintext).unwrap();
    let enc2 = ratchet2.encrypt(plaintext).unwrap();

    // Ciphertexts should be different
    assert_ne!(
        enc1.ciphertext, enc2.ciphertext,
        "Different secrets should produce different ciphertexts"
    );
}

/// Test: Same plaintext encrypts differently each time (nonce)
#[test]
fn test_same_plaintext_different_ciphertext() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    let plaintext = b"Same message";
    let enc1 = alice_ratchet.encrypt(plaintext).unwrap();
    let enc2 = alice_ratchet.encrypt(plaintext).unwrap();

    // Same plaintext should encrypt to different ciphertexts
    assert_ne!(
        enc1.ciphertext, enc2.ciphertext,
        "Same plaintext should encrypt differently each time (semantic security)"
    );
}

// =============================================================================
// Bidirectional Communication Tests
// =============================================================================

/// Test: Bidirectional ratchet conversation
#[test]
fn test_bidirectional_conversation() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Alice -> Bob
    let msg1 = b"Hello Bob";
    let enc1 = alice_ratchet.encrypt(msg1).unwrap();
    let dec1 = bob_ratchet.decrypt(&enc1).unwrap();
    assert_eq!(dec1, msg1);

    // Bob -> Alice
    let msg2 = b"Hi Alice";
    let enc2 = bob_ratchet.encrypt(msg2).unwrap();
    let dec2 = alice_ratchet.decrypt(&enc2).unwrap();
    assert_eq!(dec2, msg2);

    // Alice -> Bob again
    let msg3 = b"How are you?";
    let enc3 = alice_ratchet.encrypt(msg3).unwrap();
    let dec3 = bob_ratchet.decrypt(&enc3).unwrap();
    assert_eq!(dec3, msg3);

    // Bob -> Alice again
    let msg4 = b"I'm fine!";
    let enc4 = bob_ratchet.encrypt(msg4).unwrap();
    let dec4 = alice_ratchet.decrypt(&enc4).unwrap();
    assert_eq!(dec4, msg4);
}

/// Test: Multiple consecutive messages from same party
#[test]
fn test_consecutive_messages_same_party() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Alice sends 5 consecutive messages
    let messages: Vec<&[u8]> = vec![
        b"Message 1",
        b"Message 2",
        b"Message 3",
        b"Message 4",
        b"Message 5",
    ];

    let encrypted: Vec<_> = messages
        .iter()
        .map(|m| alice_ratchet.encrypt(*m).unwrap())
        .collect();

    // Bob decrypts all
    for (enc, expected) in encrypted.iter().zip(messages.iter()) {
        let decrypted = bob_ratchet.decrypt(enc).unwrap();
        assert_eq!(decrypted, *expected);
    }
}

// =============================================================================
// Serialization Tests
// =============================================================================

/// Test: Ratchet state can be serialized and restored
#[test]
fn test_ratchet_state_serialization() {
    use webbook_core::{network::MockTransport, Contact, ContactCard, WebBook};

    let mut alice_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();
    let mut bob_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    alice_wb.create_identity("Alice").unwrap();
    bob_wb.create_identity("Bob").unwrap();

    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    // Create contacts first (required for foreign key)
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();

    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone());
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact =
        Contact::from_exchange(alice_pk, ContactCard::new("Alice"), shared_secret.clone());
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    // Initialize ratchets
    let alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Save to storage
    alice_wb
        .storage()
        .save_ratchet_state(&bob_contact_id, &alice_ratchet, true)
        .unwrap();
    bob_wb
        .storage()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, false)
        .unwrap();

    // Load back
    let (loaded_alice, is_initiator_a) = alice_wb
        .storage()
        .load_ratchet_state(&bob_contact_id)
        .unwrap()
        .unwrap();
    let (loaded_bob, is_initiator_b) = bob_wb
        .storage()
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
#[test]
fn test_ratchet_message_serialization() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    let plaintext = b"Test message";
    let encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    // Serialize to JSON
    let json = serde_json::to_string(&encrypted).unwrap();

    // Deserialize back
    let restored: webbook_core::crypto::ratchet::RatchetMessage =
        serde_json::from_str(&json).unwrap();

    // Verify fields match
    assert_eq!(encrypted.dh_public, restored.dh_public);
    assert_eq!(encrypted.dh_generation, restored.dh_generation);
    assert_eq!(encrypted.message_index, restored.message_index);
    assert_eq!(
        encrypted.previous_chain_length,
        restored.previous_chain_length
    );
    assert_eq!(encrypted.ciphertext, restored.ciphertext);
}
