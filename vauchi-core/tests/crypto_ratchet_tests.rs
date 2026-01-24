//! Tests for crypto::ratchet
//! Extracted from ratchet.rs

use vauchi_core::crypto::*;
use vauchi_core::*;

fn create_test_pair() -> (DoubleRatchetState, DoubleRatchetState) {
    // Simulate X3DH: both parties derive the same shared secret
    let shared_secret = SymmetricKey::from_bytes([42u8; 32]);

    // Bob's initial DH keypair (used in X3DH)
    let bob_dh = X3DHKeyPair::generate();
    let bob_public = *bob_dh.public_key();

    // Alice initializes as initiator with Bob's public key
    let alice = DoubleRatchetState::initialize_initiator(&shared_secret, bob_public);

    // Bob initializes as responder with his keypair
    let bob = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    (alice, bob)
}

#[test]
fn test_dr_encrypt_decrypt_roundtrip() {
    let (mut alice, mut bob) = create_test_pair();

    // Alice sends to Bob
    let plaintext = b"Hello Bob!";
    let message = alice.encrypt(plaintext).unwrap();
    let decrypted = bob.decrypt(&message).unwrap();

    assert_eq!(plaintext.as_slice(), decrypted.as_slice());
}

#[test]
fn test_dr_bidirectional_communication() {
    let (mut alice, mut bob) = create_test_pair();

    // Alice -> Bob
    let msg1 = alice.encrypt(b"Hello Bob").unwrap();
    let dec1 = bob.decrypt(&msg1).unwrap();
    assert_eq!(b"Hello Bob".as_slice(), dec1.as_slice());

    // Bob -> Alice
    let msg2 = bob.encrypt(b"Hello Alice").unwrap();
    let dec2 = alice.decrypt(&msg2).unwrap();
    assert_eq!(b"Hello Alice".as_slice(), dec2.as_slice());

    // Alice -> Bob again
    let msg3 = alice.encrypt(b"How are you?").unwrap();
    let dec3 = bob.decrypt(&msg3).unwrap();
    assert_eq!(b"How are you?".as_slice(), dec3.as_slice());
}

#[test]
fn test_dr_forward_secrecy() {
    let (mut alice, mut bob) = create_test_pair();

    // Alice sends multiple messages
    let msg1 = alice.encrypt(b"Message 1").unwrap();
    let msg2 = alice.encrypt(b"Message 2").unwrap();

    // Bob decrypts message 1
    bob.decrypt(&msg1).unwrap();

    // Even if we had access to current keys, we can't decrypt msg1 again
    // (the key was consumed)
    // This is forward secrecy - old keys are deleted

    // But msg2 still works
    let dec2 = bob.decrypt(&msg2).unwrap();
    assert_eq!(b"Message 2".as_slice(), dec2.as_slice());
}

#[test]
fn test_dr_out_of_order_messages() {
    let (mut alice, mut bob) = create_test_pair();

    // Alice sends three messages
    let msg1 = alice.encrypt(b"First").unwrap();
    let msg2 = alice.encrypt(b"Second").unwrap();
    let msg3 = alice.encrypt(b"Third").unwrap();

    // Bob receives them out of order
    let dec3 = bob.decrypt(&msg3).unwrap();
    assert_eq!(b"Third".as_slice(), dec3.as_slice());

    let dec1 = bob.decrypt(&msg1).unwrap();
    assert_eq!(b"First".as_slice(), dec1.as_slice());

    let dec2 = bob.decrypt(&msg2).unwrap();
    assert_eq!(b"Second".as_slice(), dec2.as_slice());
}

#[test]
fn test_dr_dh_ratchet_on_reply() {
    let (mut alice, mut bob) = create_test_pair();

    let initial_alice_dh = alice.our_public_key();

    // Alice sends
    let msg1 = alice.encrypt(b"Hello").unwrap();
    bob.decrypt(&msg1).unwrap();

    // Bob replies - this triggers DH ratchet for Bob
    let msg2 = bob.encrypt(b"Hi").unwrap();
    alice.decrypt(&msg2).unwrap();

    // Alice's DH key changes when she sends again
    let _msg3 = alice.encrypt(b"Bye").unwrap();

    // Alice's DH key should have changed
    assert_ne!(initial_alice_dh, alice.our_public_key());
}

#[test]
fn test_dr_multiple_ratchets() {
    let (mut alice, mut bob) = create_test_pair();

    // Multiple back-and-forth exchanges
    for i in 0..5 {
        let msg_a = alice.encrypt(format!("Alice {}", i).as_bytes()).unwrap();
        bob.decrypt(&msg_a).unwrap();

        let msg_b = bob.encrypt(format!("Bob {}", i).as_bytes()).unwrap();
        alice.decrypt(&msg_b).unwrap();
    }

    // Both should have ratcheted multiple times
    assert!(alice.dh_generation() > 0);
    assert!(bob.dh_generation() > 0);
}

#[test]
fn test_dr_responder_cannot_send_first() {
    let shared_secret = SymmetricKey::from_bytes([42u8; 32]);
    let bob_dh = X3DHKeyPair::generate();
    let mut bob = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Bob (responder) tries to send first - should fail
    let result = bob.encrypt(b"Hello");
    assert!(result.is_err());
}

#[test]
fn test_dr_different_keys_per_message() {
    let (mut alice, _bob) = create_test_pair();

    let msg1 = alice.encrypt(b"Test 1").unwrap();
    let msg2 = alice.encrypt(b"Test 2").unwrap();

    // Ciphertexts should be different (different keys used)
    assert_ne!(msg1.ciphertext, msg2.ciphertext);

    // Message indices should increment
    assert_eq!(msg1.message_index, 0);
    assert_eq!(msg2.message_index, 1);
}

#[test]
fn test_dr_skipped_message_limit() {
    let (mut alice, mut bob) = create_test_pair();

    // Send many messages
    let mut messages = Vec::new();
    for i in 0..100 {
        messages.push(alice.encrypt(format!("Msg {}", i).as_bytes()).unwrap());
    }

    // Skip to message 99 first
    bob.decrypt(&messages[99]).unwrap();

    // This should have stored 99 skipped keys
    assert_eq!(bob.skipped_keys_count(), 99);

    // Now we can decrypt the skipped messages
    for (i, msg) in messages.iter().enumerate().take(99) {
        let dec = bob.decrypt(msg).unwrap();
        assert_eq!(format!("Msg {}", i).as_bytes(), dec.as_slice());
    }

    // Skipped keys should be consumed
    assert_eq!(bob.skipped_keys_count(), 0);
}

#[test]
fn test_dr_empty_message() {
    let (mut alice, mut bob) = create_test_pair();

    let msg = alice.encrypt(b"").unwrap();
    let dec = bob.decrypt(&msg).unwrap();

    assert!(dec.is_empty());
}

#[test]
fn test_dr_large_message() {
    let (mut alice, mut bob) = create_test_pair();

    let large_data = vec![0xABu8; 100_000];
    let msg = alice.encrypt(&large_data).unwrap();
    let dec = bob.decrypt(&msg).unwrap();

    assert_eq!(large_data, dec);
}

#[test]
fn test_ratchet_serialize_roundtrip() {
    let (alice, _bob) = create_test_pair();

    // Serialize
    let serialized = alice.serialize();

    // Deserialize
    let restored = DoubleRatchetState::deserialize(serialized).unwrap();

    // Verify state is preserved
    assert_eq!(alice.dh_generation(), restored.dh_generation());
    assert_eq!(alice.our_public_key(), restored.our_public_key());
}

#[test]
fn test_ratchet_serialize_after_messages() {
    let (mut alice, mut bob) = create_test_pair();

    // Exchange some messages
    let msg1 = alice.encrypt(b"Hello").unwrap();
    bob.decrypt(&msg1).unwrap();

    let msg2 = bob.encrypt(b"World").unwrap();
    alice.decrypt(&msg2).unwrap();

    // Serialize alice's state after messaging
    let serialized = alice.serialize();
    let mut restored = DoubleRatchetState::deserialize(serialized).unwrap();

    // The restored state should be able to continue the conversation
    let msg3 = restored.encrypt(b"Continued").unwrap();
    let decrypted = bob.decrypt(&msg3).unwrap();
    assert_eq!(b"Continued".as_slice(), decrypted.as_slice());
}

#[test]
fn test_ratchet_serialize_with_skipped_keys() {
    let (mut alice, mut bob) = create_test_pair();

    // Send messages to create skipped keys scenario
    let msg1 = alice.encrypt(b"One").unwrap();
    let msg2 = alice.encrypt(b"Two").unwrap();
    let msg3 = alice.encrypt(b"Three").unwrap();

    // Receive out of order to create skipped keys
    bob.decrypt(&msg3).unwrap();

    // Serialize bob with skipped keys
    let serialized = bob.serialize();
    let mut restored = DoubleRatchetState::deserialize(serialized).unwrap();

    // Restored should still have the skipped keys
    assert_eq!(restored.skipped_keys_count(), 2);

    // And should be able to decrypt the skipped messages
    let dec1 = restored.decrypt(&msg1).unwrap();
    let dec2 = restored.decrypt(&msg2).unwrap();
    assert_eq!(b"One".as_slice(), dec1.as_slice());
    assert_eq!(b"Two".as_slice(), dec2.as_slice());
}
