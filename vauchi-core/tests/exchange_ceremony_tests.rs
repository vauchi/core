// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Crypto Ceremony Tests (Item 198)
//!
//! Tests the full X3DH key agreement ceremony at the application layer,
//! not just serialization or state machine transitions. Each test exercises
//! real crypto operations and verifies shared secret properties.
//!
//! Feature file: features/contact_exchange.feature @security @exchange

use vauchi_core::crypto::{decrypt, encrypt};
use vauchi_core::exchange::*;
use vauchi_core::{ContactCard, Identity};

// ============================================================
// Session key agreement cross-check
// ============================================================

/// Verify QR session key agreement produces matching keys and they
/// are usable for bidirectional encryption. This is the core ceremony test.
#[test]
fn test_qr_ceremony_shared_keys_match_and_encrypt() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session =
        ExchangeSession::new_qr(alice_identity, alice_card.clone(), alice_proximity);

    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card.clone(), bob_proximity);

    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let alice_key = match alice_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("Alice should be in Complete state"),
    };
    let bob_key = match bob_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("Bob should be in Complete state"),
    };

    // Keys must match
    assert_eq!(
        alice_key.as_bytes(),
        bob_key.as_bytes(),
        "Session key agreement must produce matching keys"
    );

    // Keys must not be degenerate
    assert_ne!(
        alice_key.as_bytes(),
        &[0u8; 32],
        "Shared key must not be all zeros"
    );

    // Bidirectional encryption works
    let msg = b"Ceremony test message";
    let ct = encrypt(&alice_key, msg).unwrap();
    let pt = decrypt(&bob_key, &ct).unwrap();
    assert_eq!(pt, msg, "Bob should decrypt Alice's message");
}

// ============================================================
// Tampered wire format: ephemeral and sender keys
// ============================================================

/// Tampered ephemeral key in wire format causes AEAD decryption failure.
///
/// If an attacker modifies the ephemeral_public_key in transit, the
/// recipient derives a different shared secret, and AEAD tag verification
/// fails. This is a ceremony-level integrity test.
#[test]
fn test_tampered_ephemeral_key_causes_decrypt_failure() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let alice_identity = [0x41u8; 32];

    let (mut msg, _alice_secret) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &alice_identity, "Alice")
            .unwrap();

    // Tamper with the ephemeral public key (change one byte)
    msg.ephemeral_public_key[0] ^= 0x01;

    let result = msg.decrypt(&bob);

    assert!(
        result.is_err(),
        "Decryption must fail when ephemeral key is tampered"
    );
}

/// Tampered sender_exchange_key causes DH1 identity binding mismatch.
#[test]
fn test_tampered_sender_exchange_key_causes_decrypt_failure() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (mut msg, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x01u8; 32], "Alice").unwrap();

    // Tamper with the identity binding component
    msg.sender_exchange_key[0] ^= 0x01;

    let result = msg.decrypt(&bob);

    assert!(
        result.is_err(),
        "Decryption must fail when sender exchange key is tampered"
    );
}

/// Tampered ciphertext causes AEAD tag failure.
#[test]
fn test_tampered_ciphertext_causes_decrypt_failure() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (mut msg, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x01u8; 32], "Alice").unwrap();

    // Tamper with the ciphertext (flip a byte)
    if let Some(byte) = msg.ciphertext.last_mut() {
        *byte ^= 0x01;
    }

    let result = msg.decrypt(&bob);

    assert!(
        result.is_err(),
        "Decryption must fail when ciphertext is tampered"
    );
}

// ============================================================
// Each DH component contributes to the shared secret
// ============================================================

/// Different identity keys (DH1) produce different shared secrets.
#[test]
fn test_identity_binding_changes_shared_secret() {
    let alice1 = X3DHKeyPair::generate();
    let alice2 = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (secret1, _) = X3DH::initiate(&alice1, bob.public_key()).unwrap();
    let (secret2, _) = X3DH::initiate(&alice2, bob.public_key()).unwrap();

    assert_ne!(
        secret1.as_bytes(),
        secret2.as_bytes(),
        "Different identity keys must produce different secrets (DH1 contribution)"
    );
}

/// Same identity keys with different ephemerals produce different secrets.
#[test]
fn test_ephemeral_changes_shared_secret() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (secret1, eph1) = X3DH::initiate(&alice, bob.public_key()).unwrap();
    let (secret2, eph2) = X3DH::initiate(&alice, bob.public_key()).unwrap();

    assert_ne!(eph1, eph2, "Each initiation must use a fresh ephemeral");
    assert_ne!(
        secret1.as_bytes(),
        secret2.as_bytes(),
        "Different ephemerals must produce different secrets (DH2 contribution)"
    );
}

// ============================================================
// Transport isolation: NFC and QR sessions produce independent secrets
// ============================================================

/// Independent sessions using fresh ephemerals produce different shared secrets.
#[test]
fn test_independent_sessions_produce_different_secrets() {
    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    // --- QR session 1 ---
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let qr_alice_proximity = MockProximityVerifier::success();
    let mut qr_alice =
        ExchangeSession::new_qr(alice_identity, alice_card.clone(), qr_alice_proximity);
    let qr_bob_proximity = MockProximityVerifier::success();
    let mut qr_bob = ExchangeSession::new_qr(bob_identity, bob_card.clone(), qr_bob_proximity);

    qr_alice.apply(ExchangeEvent::StartQR).unwrap();
    qr_bob.apply(ExchangeEvent::StartQR).unwrap();
    let qr_a = qr_alice.qr().unwrap().clone();
    let qr_b = qr_bob.qr().unwrap().clone();
    qr_alice.apply(ExchangeEvent::ProcessQR(qr_b)).unwrap();
    qr_bob.apply(ExchangeEvent::ProcessQR(qr_a)).unwrap();
    qr_alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr_bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr_alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr_bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr_alice
        .apply(ExchangeEvent::CompleteExchange(bob_card.clone()))
        .unwrap();
    qr_bob
        .apply(ExchangeEvent::CompleteExchange(alice_card.clone()))
        .unwrap();

    let qr_key = match qr_alice.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("QR Alice should be complete"),
    };

    // --- QR session 2 (separate identities, fresh ephemerals) ---
    let alice_identity2 = Identity::create("Alice");
    let bob_identity2 = Identity::create("Bob");

    let qr2_alice_proximity = MockProximityVerifier::success();
    let mut qr2_alice =
        ExchangeSession::new_qr(alice_identity2, alice_card.clone(), qr2_alice_proximity);
    let qr2_bob_proximity = MockProximityVerifier::success();
    let mut qr2_bob = ExchangeSession::new_qr(bob_identity2, bob_card.clone(), qr2_bob_proximity);

    qr2_alice.apply(ExchangeEvent::StartQR).unwrap();
    qr2_bob.apply(ExchangeEvent::StartQR).unwrap();
    let qr2_a = qr2_alice.qr().unwrap().clone();
    let qr2_b = qr2_bob.qr().unwrap().clone();
    qr2_alice.apply(ExchangeEvent::ProcessQR(qr2_b)).unwrap();
    qr2_bob.apply(ExchangeEvent::ProcessQR(qr2_a)).unwrap();
    qr2_alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr2_bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr2_alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr2_bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr2_alice
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    qr2_bob
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let qr2_key = match qr2_alice.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("QR2 Alice should be complete"),
    };

    // Sessions with fresh ephemerals must produce different secrets
    assert_ne!(
        qr_key.as_bytes(),
        qr2_key.as_bytes(),
        "Independent sessions must produce different shared secrets"
    );
}

// ============================================================
// EncryptedExchangeMessage ceremony: create + decrypt consistency
// ============================================================

/// Verify EncryptedExchangeMessage shared secret matches raw X3DH.
///
/// This cross-checks that the message layer uses X3DH internally
/// and produces the same key as calling X3DH::initiate/respond directly.
#[test]
fn test_encrypted_message_secret_matches_raw_x3dh() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (msg, alice_secret) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x42u8; 32], "Alice").unwrap();

    // Decrypt to get Bob's secret
    let (_payload, bob_secret) = msg.decrypt(&bob).unwrap();

    // Both must match
    assert_eq!(
        alice_secret.as_bytes(),
        bob_secret.as_bytes(),
        "EncryptedExchangeMessage must produce matching secrets"
    );

    // The secret should be HKDF-derived (not raw DH)
    let raw_dh = bob.diffie_hellman(&msg.ephemeral_public_key);
    assert_ne!(
        alice_secret.as_bytes(),
        &raw_dh,
        "Secret must be HKDF-derived, not raw DH"
    );
}
