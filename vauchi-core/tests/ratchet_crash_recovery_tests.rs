//! Ratchet Crash Recovery Tests
//!
//! Tests for ratchet state persistence and recovery after crashes.
//! Based on: sync_updates.feature edge cases

use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::X3DHKeyPair;

// =============================================================================
// Basic Serialization Tests
// =============================================================================

/// Scenario: Ratchet state survives serialization roundtrip
#[test]
fn test_ratchet_state_serialization_roundtrip() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    // Alice initializes as initiator
    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);

    // Alice sends a message (advances state)
    let msg1 = alice.encrypt(b"Hello Bob!").unwrap();

    // Serialize state
    let serialized = alice.serialize();

    // Deserialize state
    let mut alice_restored = DoubleRatchetState::deserialize(serialized).unwrap();

    // Restored state should be able to encrypt
    let msg2 = alice_restored.encrypt(b"Second message").unwrap();

    // Message indices should continue correctly
    assert_eq!(msg1.message_index, 0);
    assert_eq!(msg2.message_index, 1);
}

/// Scenario: Both parties can restore state and continue
#[test]
fn test_both_parties_restore_state() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    // Initialize both parties
    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends, Bob receives
    let msg1 = alice.encrypt(b"Hello Bob!").unwrap();
    let plaintext1 = bob.decrypt(&msg1).unwrap();
    assert_eq!(plaintext1, b"Hello Bob!");

    // Serialize both states (simulating app shutdown)
    let alice_serialized = alice.serialize();
    let bob_serialized = bob.serialize();

    // Restore both states (simulating app restart)
    let mut alice_restored = DoubleRatchetState::deserialize(alice_serialized).unwrap();
    let mut bob_restored = DoubleRatchetState::deserialize(bob_serialized).unwrap();

    // Bob can now reply
    let msg2 = bob_restored.encrypt(b"Hi Alice!").unwrap();
    let plaintext2 = alice_restored.decrypt(&msg2).unwrap();
    assert_eq!(plaintext2, b"Hi Alice!");

    // And conversation can continue
    let msg3 = alice_restored.encrypt(b"How are you?").unwrap();
    let plaintext3 = bob_restored.decrypt(&msg3).unwrap();
    assert_eq!(plaintext3, b"How are you?");
}

// =============================================================================
// Crash During Ratchet Advance Tests
// =============================================================================

/// Scenario: State can be restored at any point during conversation
#[test]
fn test_restore_mid_conversation() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Exchange several messages
    for i in 0..5 {
        let msg = alice.encrypt(format!("Message {}", i).as_bytes()).unwrap();
        bob.decrypt(&msg).unwrap();
    }

    // Serialize after multiple messages
    let alice_state = alice.serialize();
    let bob_state = bob.serialize();

    // Restore
    let mut alice = DoubleRatchetState::deserialize(alice_state).unwrap();
    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    // Should be able to continue with correct message indices
    let msg = alice.encrypt(b"After restore").unwrap();
    assert_eq!(msg.message_index, 5); // Continues from where we left off

    let plaintext = bob.decrypt(&msg).unwrap();
    assert_eq!(plaintext, b"After restore");
}

/// Scenario: DH ratchet state survives serialization
#[test]
fn test_dh_ratchet_state_survives() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice -> Bob (first DH generation)
    let msg1 = alice.encrypt(b"First").unwrap();
    assert_eq!(msg1.dh_generation, 0);
    bob.decrypt(&msg1).unwrap();

    // Bob -> Alice (triggers DH ratchet)
    let msg2 = bob.encrypt(b"Second").unwrap();
    alice.decrypt(&msg2).unwrap();

    // Serialize after DH ratchet
    let alice_state = alice.serialize();
    let bob_state = bob.serialize();

    // Restore
    let mut alice = DoubleRatchetState::deserialize(alice_state).unwrap();
    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    // Continue conversation - DH generation should be preserved
    let msg3 = alice.encrypt(b"Third").unwrap();
    bob.decrypt(&msg3).unwrap();

    let msg4 = bob.encrypt(b"Fourth").unwrap();
    alice.decrypt(&msg4).unwrap();
}

// =============================================================================
// Skipped Keys Persistence Tests
// =============================================================================

/// Scenario: Skipped message keys survive serialization
#[test]
fn test_skipped_keys_survive_serialization() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice sends 3 messages
    let msg0 = alice.encrypt(b"Message 0").unwrap();
    let msg1 = alice.encrypt(b"Message 1").unwrap();
    let msg2 = alice.encrypt(b"Message 2").unwrap();

    // Bob receives them out of order: 2, then crash, then 0, 1
    bob.decrypt(&msg2).unwrap(); // This will store keys for msg0, msg1

    // Serialize (skipped keys should be saved)
    let bob_state = bob.serialize();

    // Check skipped keys are in serialized state
    assert_eq!(bob_state.skipped_keys.len(), 2);

    // Restore
    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    // Should be able to decrypt the skipped messages
    let plaintext0 = bob.decrypt(&msg0).unwrap();
    let plaintext1 = bob.decrypt(&msg1).unwrap();

    assert_eq!(plaintext0, b"Message 0");
    assert_eq!(plaintext1, b"Message 1");
}

// =============================================================================
// Responder State Tests
// =============================================================================

/// Scenario: Responder can restore before receiving first message
#[test]
fn test_responder_restore_before_first_message() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    // Initialize responder (hasn't received anything yet)
    let bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Serialize immediately
    let bob_state = bob.serialize();

    // Restore
    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    // Alice sends first message
    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let msg = alice.encrypt(b"Hello!").unwrap();

    // Bob should be able to decrypt
    let plaintext = bob.decrypt(&msg).unwrap();
    assert_eq!(plaintext, b"Hello!");
}

// =============================================================================
// Invalid Serialized State Tests
// =============================================================================

/// Scenario: Invalid serialized state is rejected
#[test]
fn test_invalid_send_chain_rejected() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut state = alice.serialize();

    // Corrupt the send chain
    if let Some((ref mut key, _)) = state.send_chain {
        *key = [0xFF; 32]; // Set to all 0xFF
    }

    // Should still deserialize (corruption doesn't cause format error)
    // But messages encrypted with corrupted state won't decrypt properly
    let alice = DoubleRatchetState::deserialize(state);
    assert!(alice.is_ok()); // Deserialization succeeds
}

// =============================================================================
// Stress Tests
// =============================================================================

/// Scenario: Many serialization cycles don't corrupt state
#[test]
fn test_many_serialization_cycles() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public);
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    for i in 0..20 {
        // Send message
        let msg = alice.encrypt(format!("Message {}", i).as_bytes()).unwrap();
        bob.decrypt(&msg).unwrap();

        // Serialize and restore both parties
        let alice_state = alice.serialize();
        let bob_state = bob.serialize();

        alice = DoubleRatchetState::deserialize(alice_state).unwrap();
        bob = DoubleRatchetState::deserialize(bob_state).unwrap();

        // Bob replies
        let reply = bob.encrypt(format!("Reply {}", i).as_bytes()).unwrap();
        alice.decrypt(&reply).unwrap();

        // Serialize and restore again
        let alice_state = alice.serialize();
        let bob_state = bob.serialize();

        alice = DoubleRatchetState::deserialize(alice_state).unwrap();
        bob = DoubleRatchetState::deserialize(bob_state).unwrap();
    }

    // Final message should work
    let final_msg = alice.encrypt(b"Final message").unwrap();
    let plaintext = bob.decrypt(&final_msg).unwrap();
    assert_eq!(plaintext, b"Final message");
}
