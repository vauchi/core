// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for exchange::encrypted_message
//! Extracted from encrypted_message.rs

use vauchi_core::exchange::*;

// @scenario: contact_exchange :: X3DH key agreement during exchange
// @internal
#[test]
fn test_encrypted_message_basic_roundtrip() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let alice_identity_key = [0x41u8; 32];
    let alice_name = "Alice";

    let (msg, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &alice_identity_key, alice_name)
            .unwrap();

    let (payload, _shared_secret) = msg.decrypt(&bob).unwrap();

    assert_eq!(payload.identity_key, alice_identity_key);
    assert_eq!(payload.exchange_key, *alice.public_key());
    assert_eq!(payload.display_name, alice_name);
}

/// Wire format includes sender_exchange_key for identity binding.
// @scenario: contact_exchange :: Exchange verifies identity
// @internal
#[test]
fn test_encrypted_message_contains_sender_exchange_key() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (msg, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x01u8; 32], "Alice").unwrap();

    assert_eq!(
        msg.sender_exchange_key,
        *alice.public_key(),
        "Wire format must contain sender's exchange key"
    );
}

/// Full roundtrip with identity binding: create + decrypt produces matching keys.
// @scenario: contact_exchange :: Exchange creates mutual keys
// @internal
#[test]
fn test_encrypted_message_roundtrip_with_identity_binding() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let alice_identity = [0x02u8; 32];
    let (msg, alice_secret) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &alice_identity, "Alice")
            .unwrap();

    let (payload, bob_secret) = msg.decrypt(&bob).unwrap();

    // Payload correctly recovered
    assert_eq!(payload.identity_key, alice_identity);
    assert_eq!(payload.exchange_key, *alice.public_key());
    assert_eq!(payload.display_name, "Alice");

    // Both sides derived the same shared secret
    assert_eq!(
        alice_secret.as_bytes(),
        bob_secret.as_bytes(),
        "Identity-bound shared secrets must match"
    );
}

/// Serialization roundtrip preserves sender_exchange_key.
// @internal
#[test]
fn test_encrypted_message_serialization_preserves_exchange_key() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (original, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x03u8; 32], "Test").unwrap();

    let bytes = original.to_bytes().unwrap();
    let restored = EncryptedExchangeMessage::from_bytes(&bytes).unwrap();

    assert_eq!(restored.sender_exchange_key, original.sender_exchange_key);
    assert_eq!(restored.ephemeral_public_key, original.ephemeral_public_key);
    assert_eq!(restored.ciphertext, original.ciphertext);
}
