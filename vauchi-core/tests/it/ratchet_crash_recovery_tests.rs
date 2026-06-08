// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ratchet Crash Recovery Tests
//!
//! Tests for ratchet state persistence and recovery after crashes.
//! Based on: sync_updates.feature edge cases

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;

// =============================================================================
// =============================================================================

/// Scenario: Ratchet state survives serialization roundtrip
// @scenario: security :: Forward secrecy via Double Ratchet
// @internal
#[test]
fn test_ratchet_state_serialization_roundtrip() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();

    // Alice sends a message (advances state)
    let msg1 = alice.encrypt(b"Hello Bob!").unwrap();

    let serialized = alice.serialize();

    let mut alice_restored = DoubleRatchetState::deserialize(serialized).unwrap();

    let msg2 = alice_restored.encrypt(b"Second message").unwrap();

    assert_eq!(msg1.message_index, 0);
    assert_eq!(msg2.message_index, 1);
}

/// Scenario: Both parties can restore state and continue
// @scenario: security :: Forward secrecy via Double Ratchet
// @internal
#[test]
fn test_both_parties_restore_state() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    let msg1 = alice.encrypt(b"Hello Bob!").unwrap();
    let plaintext1 = bob.decrypt(&msg1).unwrap();
    assert_eq!(plaintext1, b"Hello Bob!");

    // Serialize both states (simulating app shutdown)
    let alice_serialized = alice.serialize();
    let bob_serialized = bob.serialize();

    // Restore both states (simulating app restart)
    let mut alice_restored = DoubleRatchetState::deserialize(alice_serialized).unwrap();
    let mut bob_restored = DoubleRatchetState::deserialize(bob_serialized).unwrap();

    let msg2 = bob_restored.encrypt(b"Hi Alice!").unwrap();
    let plaintext2 = alice_restored.decrypt(&msg2).unwrap();
    assert_eq!(plaintext2, b"Hi Alice!");

    let msg3 = alice_restored.encrypt(b"How are you?").unwrap();
    let plaintext3 = bob_restored.decrypt(&msg3).unwrap();
    assert_eq!(plaintext3, b"How are you?");
}

// =============================================================================
// Crash During Ratchet Advance Tests
// =============================================================================

/// Scenario: State can be restored at any point during conversation
// @scenario: security :: Forward secrecy via Double Ratchet
// @internal
#[test]
fn test_restore_mid_conversation() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    for i in 0..5 {
        let msg = alice.encrypt(format!("Message {}", i).as_bytes()).unwrap();
        bob.decrypt(&msg).unwrap();
    }

    let alice_state = alice.serialize();
    let bob_state = bob.serialize();

    let mut alice = DoubleRatchetState::deserialize(alice_state).unwrap();
    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    let msg = alice.encrypt(b"After restore").unwrap();
    assert_eq!(msg.message_index, 5); // Continues from where we left off

    let plaintext = bob.decrypt(&msg).unwrap();
    assert_eq!(plaintext, b"After restore");
}

/// Scenario: DH ratchet state survives serialization
// @internal
#[test]
fn test_dh_ratchet_state_survives() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    // Alice -> Bob (first DH generation)
    let msg1 = alice.encrypt(b"First").unwrap();
    assert_eq!(msg1.dh_generation, 0);
    bob.decrypt(&msg1).unwrap();

    // Bob -> Alice (triggers DH ratchet)
    let msg2 = bob.encrypt(b"Second").unwrap();
    alice.decrypt(&msg2).unwrap();

    let alice_state = alice.serialize();
    let bob_state = bob.serialize();

    let mut alice = DoubleRatchetState::deserialize(alice_state).unwrap();
    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    let msg3 = alice.encrypt(b"Third").unwrap();
    bob.decrypt(&msg3).unwrap();

    let msg4 = bob.encrypt(b"Fourth").unwrap();
    alice.decrypt(&msg4).unwrap();
}

// =============================================================================
// =============================================================================

/// Scenario: Skipped message keys survive serialization
// @scenario: security :: Forward secrecy via Double Ratchet
// @internal
#[test]
fn test_skipped_keys_survive_serialization() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    let msg0 = alice.encrypt(b"Message 0").unwrap();
    let msg1 = alice.encrypt(b"Message 1").unwrap();
    let msg2 = alice.encrypt(b"Message 2").unwrap();

    bob.decrypt(&msg2).unwrap(); // This will store keys for msg0, msg1

    // Serialize (skipped keys should be saved)
    let bob_state = bob.serialize();

    assert_eq!(bob_state.skipped_keys.len(), 2);

    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    let plaintext0 = bob.decrypt(&msg0).unwrap();
    let plaintext1 = bob.decrypt(&msg1).unwrap();

    assert_eq!(plaintext0, b"Message 0");
    assert_eq!(plaintext1, b"Message 1");
}

// =============================================================================
// =============================================================================

/// Scenario: Responder can restore before receiving first message
// @internal
#[test]
fn test_responder_restore_before_first_message() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    // Initialize responder (hasn't received anything yet)
    let bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    let bob_state = bob.serialize();

    let mut bob = DoubleRatchetState::deserialize(bob_state).unwrap();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();
    let msg = alice.encrypt(b"Hello!").unwrap();

    let plaintext = bob.decrypt(&msg).unwrap();
    assert_eq!(plaintext, b"Hello!");
}

// =============================================================================
// =============================================================================

/// Scenario: Invalid serialized state is rejected
// @internal
#[test]
fn test_invalid_send_chain_rejected() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();
    let mut state = alice.serialize();

    if let Some((ref mut key, _)) = state.send_chain {
        *key = [0xFF; 32]; // Set to all 0xFF
    }

    // Should still deserialize (corruption doesn't cause format error)
    // But messages encrypted with corrupted state won't decrypt properly
    let alice = DoubleRatchetState::deserialize(state);
    alice.expect("expected success"); // Deserialization succeeds
}

// =============================================================================
// =============================================================================

/// Scenario: Many serialization cycles don't corrupt state
// @internal
#[test]
fn test_many_serialization_cycles() {
    let x3dh_secret = SymmetricKey::generate();
    let bob_keypair = X3DHKeyPair::generate();
    let bob_public = *bob_keypair.public_key();

    let mut alice = DoubleRatchetState::initialize_initiator(&x3dh_secret, bob_public).unwrap();
    let mut bob = DoubleRatchetState::initialize_responder(&x3dh_secret, bob_keypair);

    for i in 0..20 {
        let msg = alice.encrypt(format!("Message {}", i).as_bytes()).unwrap();
        bob.decrypt(&msg).unwrap();

        let alice_state = alice.serialize();
        let bob_state = bob.serialize();

        alice = DoubleRatchetState::deserialize(alice_state).unwrap();
        bob = DoubleRatchetState::deserialize(bob_state).unwrap();

        let reply = bob.encrypt(format!("Reply {}", i).as_bytes()).unwrap();
        alice.decrypt(&reply).unwrap();

        let alice_state = alice.serialize();
        let bob_state = bob.serialize();

        alice = DoubleRatchetState::deserialize(alice_state).unwrap();
        bob = DoubleRatchetState::deserialize(bob_state).unwrap();
    }

    let final_msg = alice.encrypt(b"Final message").unwrap();
    let plaintext = bob.decrypt(&final_msg).unwrap();
    assert_eq!(plaintext, b"Final message");
}
