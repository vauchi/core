//! Replay Detection Tests
//!
//! Tests for detecting and rejecting replayed messages.
//! Based on: security.feature - Replay attack prevention

use vauchi_core::crypto::ratchet::{DoubleRatchetState, RatchetError};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::X3DHKeyPair;

// =============================================================================
// Message Nonce/Index Based Replay Detection
// =============================================================================

/// Scenario: Same message cannot be decrypted twice
#[test]
fn test_same_message_decrypted_once() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends a message
    let msg = alice.encrypt(b"Secret message").unwrap();

    // Bob decrypts it once - succeeds
    let plaintext = bob.decrypt(&msg).unwrap();
    assert_eq!(plaintext, b"Secret message");

    // Bob tries to decrypt the same message again - should fail
    // The ratchet has advanced, so the key for this message_index is gone
    let result = bob.decrypt(&msg);

    // This should fail because the chain has already advanced
    // Note: The exact error depends on implementation - it might be InvalidMessage
    // or the decryption might fail (wrong key)
    assert!(
        result.is_err() || result.as_ref().unwrap() != b"Secret message",
        "Replay should not succeed with same plaintext"
    );
}

/// Scenario: Replayed message with same index fails after chain advances
#[test]
fn test_replay_after_chain_advance() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends message 0
    let msg0 = alice.encrypt(b"Message 0").unwrap();
    assert_eq!(msg0.message_index, 0);

    // Bob decrypts it
    bob.decrypt(&msg0).unwrap();

    // Alice sends more messages (advances chain)
    let msg1 = alice.encrypt(b"Message 1").unwrap();
    let msg2 = alice.encrypt(b"Message 2").unwrap();

    // Bob decrypts them
    bob.decrypt(&msg1).unwrap();
    bob.decrypt(&msg2).unwrap();

    // Attacker replays msg0
    let result = bob.decrypt(&msg0);

    // Should fail - the key for index 0 is gone
    assert!(result.is_err());
}

/// Scenario: Out-of-order messages work, but duplicates don't
#[test]
fn test_out_of_order_vs_duplicate() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends 3 messages
    let msg0 = alice.encrypt(b"Message 0").unwrap();
    let msg1 = alice.encrypt(b"Message 1").unwrap();
    let msg2 = alice.encrypt(b"Message 2").unwrap();

    // Bob receives them out of order: 2, 0, 1
    let p2 = bob.decrypt(&msg2).unwrap();
    assert_eq!(p2, b"Message 2");

    let p0 = bob.decrypt(&msg0).unwrap();
    assert_eq!(p0, b"Message 0");

    let p1 = bob.decrypt(&msg1).unwrap();
    assert_eq!(p1, b"Message 1");

    // Now all are decrypted. Try to replay msg1.
    let replay_result = bob.decrypt(&msg1);

    // Should fail - already decrypted
    assert!(replay_result.is_err());
}

// =============================================================================
// DH Generation Based Replay Detection
// =============================================================================

/// Scenario: Message from old DH generation rejected after ratchet
#[test]
fn test_old_dh_generation_rejected() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends (DH gen 0)
    let old_msg = alice.encrypt(b"Old message").unwrap();
    assert_eq!(old_msg.dh_generation, 0);

    // Bob receives
    bob.decrypt(&old_msg).unwrap();

    // Bob replies (triggers DH ratchet)
    let reply = bob.encrypt(b"Reply").unwrap();
    alice.decrypt(&reply).unwrap();

    // Alice sends new message (DH gen 1)
    let new_msg = alice.encrypt(b"New message").unwrap();
    assert!(new_msg.dh_generation > 0);

    // Bob receives new message (advances to new DH gen)
    bob.decrypt(&new_msg).unwrap();

    // Attacker replays old_msg (DH gen 0, already processed)
    // The skipped key for this was already used
    let result = bob.decrypt(&old_msg);
    assert!(result.is_err());
}

// =============================================================================
// Skipped Message Key Management
// =============================================================================

/// Scenario: Skipped keys are deleted after use
#[test]
fn test_skipped_keys_deleted_after_use() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends 3 messages
    let msg0 = alice.encrypt(b"Message 0").unwrap();
    let msg1 = alice.encrypt(b"Message 1").unwrap();
    let msg2 = alice.encrypt(b"Message 2").unwrap();

    // Bob receives msg2 first (skips 0 and 1)
    bob.decrypt(&msg2).unwrap();

    // Bob receives msg0 (uses skipped key)
    bob.decrypt(&msg0).unwrap();

    // Replay msg0 - skipped key is now deleted
    let result = bob.decrypt(&msg0);
    assert!(result.is_err());

    // msg1 should still work (skipped key still exists)
    let p1 = bob.decrypt(&msg1).unwrap();
    assert_eq!(p1, b"Message 1");

    // But replaying msg1 fails
    let result = bob.decrypt(&msg1);
    assert!(result.is_err());
}

/// Scenario: Too many skipped messages rejected (DoS protection)
#[test]
fn test_too_many_skipped_messages() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends many messages
    let mut messages = Vec::new();
    for i in 0..1100 {
        messages.push(alice.encrypt(format!("Message {}", i).as_bytes()).unwrap());
    }

    // Bob tries to receive the last message (would skip 1099 messages)
    // This exceeds MAX_SKIPPED_KEYS (1000)
    let result = bob.decrypt(&messages[1099]);

    // Should fail with TooManySkipped
    assert!(matches!(result, Err(RatchetError::TooManySkipped)));
}

// =============================================================================
// Timestamp Independence Tests
// =============================================================================

/// Scenario: Replay detection doesn't rely on timestamps alone
/// (Important: nonce/index based detection works regardless of clock)
#[test]
fn test_replay_detection_timestamp_independent() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends a message
    let msg = alice.encrypt(b"Test message").unwrap();

    // Bob decrypts it
    bob.decrypt(&msg).unwrap();

    // Even if attacker could manipulate timestamps, the message_index
    // and chain key mechanism prevents replay
    // The message has message_index=0, which is now consumed

    // Replay attempt (regardless of any timestamp manipulation)
    let result = bob.decrypt(&msg);
    assert!(result.is_err());
}

// =============================================================================
// Cross-Chain Replay Prevention
// =============================================================================

/// Scenario: Message from one chain can't be replayed on another
#[test]
fn test_cross_chain_replay_prevention() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Exchange messages to advance DH ratchet multiple times
    let msg1 = alice.encrypt(b"Alice 1").unwrap();
    bob.decrypt(&msg1).unwrap();

    let reply1 = bob.encrypt(b"Bob 1").unwrap();
    alice.decrypt(&reply1).unwrap();

    let msg2 = alice.encrypt(b"Alice 2").unwrap();
    bob.decrypt(&msg2).unwrap();

    // Capture reply1's bytes
    let replay_msg = reply1.clone();

    // More exchanges
    let reply2 = bob.encrypt(b"Bob 2").unwrap();
    alice.decrypt(&reply2).unwrap();

    // Try to replay Bob's old message to Alice
    // Alice has already processed it and advanced
    let result = alice.decrypt(&replay_msg);
    assert!(result.is_err());
}
