// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-End Exchange Tests
//!
//! Tests the complete contact exchange flow from QR generation
//! through key agreement to card exchange.
//!
//! Feature file: features/contact_exchange.feature @e2e

use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, MockProximityVerifier, X3DHKeyPair,
    X3DH,
};
use vauchi_core::{ContactCard, Identity};

// ============================================================
// Full Exchange Flow Tests
// Feature: contact_exchange.feature @e2e
// ============================================================

/// Test: Complete exchange produces matching shared keys
///
/// This verifies that:
/// 1. Alice (QR displayer) and Bob (QR scanner) complete the exchange
/// 2. Both derive the SAME shared key for encryption
/// 3. Messages encrypted by one can be decrypted by the other
#[test]
fn test_full_exchange_produces_matching_shared_keys() {
    use vauchi_core::crypto::{decrypt, encrypt};

    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    // Alice is initiator (displays QR)
    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session =
        ExchangeSession::new_initiator(alice_identity, alice_card.clone(), alice_proximity);

    // Bob is responder (scans QR)
    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session =
        ExchangeSession::new_responder(bob_identity, bob_card.clone(), bob_proximity);

    // Step 1: Alice generates QR
    alice_session.apply(ExchangeEvent::GenerateQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    // Step 2: Bob processes Alice's QR
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Step 3: Both verify proximity
    alice_session.apply(ExchangeEvent::VerifyProximity).unwrap();
    bob_session.apply(ExchangeEvent::VerifyProximity).unwrap();

    // Step 4: Key agreement with ephemeral transfer
    // Bob (scanner/X3DH initiator) performs key agreement first - generates ephemeral
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Transfer Bob's ephemeral to Alice
    let bob_ephemeral = bob_session
        .ephemeral_public()
        .expect("Scanner should have ephemeral");
    alice_session.set_their_ephemeral(bob_ephemeral);

    // Now Alice (displayer/X3DH responder) can perform key agreement
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Step 5: Complete exchange
    let _alice_contact = alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card.clone()))
        .ok();
    let _bob_contact = bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card.clone()))
        .ok();

    // Both should have completed
    assert!(
        matches!(alice_session.state(), ExchangeState::Complete { .. }),
        "Alice should be in Complete state"
    );
    assert!(
        matches!(bob_session.state(), ExchangeState::Complete { .. }),
        "Bob should be in Complete state"
    );

    // Get the shared keys from completed contacts
    let alice_shared_key = match alice_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("Alice should be complete"),
    };
    let bob_shared_key = match bob_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("Bob should be complete"),
    };

    // CRITICAL: Both should have derived the SAME shared key
    assert_eq!(
        alice_shared_key.as_bytes(),
        bob_shared_key.as_bytes(),
        "Alice and Bob should have the same shared key"
    );

    // Verify encryption/decryption works bidirectionally
    let message = b"Hello from Alice!";
    let ciphertext = encrypt(&alice_shared_key, message).unwrap();
    let decrypted = decrypt(&bob_shared_key, &ciphertext).unwrap();
    assert_eq!(decrypted, message, "Bob should decrypt Alice's message");

    let message2 = b"Hello from Bob!";
    let ciphertext2 = encrypt(&bob_shared_key, message2).unwrap();
    let decrypted2 = decrypt(&alice_shared_key, &ciphertext2).unwrap();
    assert_eq!(decrypted2, message2, "Alice should decrypt Bob's message");
}

/// Test: X3DH role mapping is correct
///
/// The QR scanner (ExchangeRole::Responder) should be the X3DH initiator
/// The QR displayer (ExchangeRole::Initiator) should be the X3DH responder
#[test]
fn test_x3dh_role_mapping() {
    let alice_keys = X3DHKeyPair::generate(); // QR displayer
    let bob_keys = X3DHKeyPair::generate(); // QR scanner

    // Bob (scanner) initiates X3DH with Alice's exchange key
    let (bob_secret, bob_ephemeral) =
        X3DH::initiate(&bob_keys, alice_keys.public_key()).expect("Bob initiates X3DH");

    // Alice (displayer) responds using Bob's ephemeral
    let alice_secret = X3DH::respond(&alice_keys, bob_keys.public_key(), &bob_ephemeral)
        .expect("Alice responds to X3DH");

    // Both should have the same secret
    assert_eq!(
        bob_secret.as_bytes(),
        alice_secret.as_bytes(),
        "Scanner and displayer should derive same key"
    );
}

/// Test: QR code contains exchange key for X3DH
#[test]
fn test_qr_contains_exchange_key() {
    let identity = Identity::create("Alice");
    let qr = ExchangeQR::generate(&identity);

    // QR should have exchange key (X25519) separate from signing key (Ed25519)
    let exchange_key = qr.exchange_key();
    let signing_key = qr.public_key();

    assert_ne!(
        exchange_key, signing_key,
        "Exchange key should be different from signing key"
    );

    // Exchange key should match identity's X25519 key
    assert_eq!(exchange_key, identity.exchange_public_key());
}

// Note: test_ephemeral_key_transfer requires ephemeral_public() and set_their_ephemeral()
// methods to be added to ExchangeSession. This will be implemented after fixing the
// basic key agreement flow.

// ============================================================
// Edge Cases
// ============================================================

/// Test: Key agreement fails with wrong ephemeral
#[test]
fn test_wrong_ephemeral_produces_different_key() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    // Bob initiates
    let (bob_secret, _correct_ephemeral) =
        X3DH::initiate(&bob_keys, alice_keys.public_key()).unwrap();

    // Alice responds with WRONG ephemeral (simulating attack/bug)
    let wrong_ephemeral = [0u8; 32];
    let alice_secret = X3DH::respond(&alice_keys, bob_keys.public_key(), &wrong_ephemeral).unwrap();

    // Keys should NOT match
    assert_ne!(
        bob_secret.as_bytes(),
        alice_secret.as_bytes(),
        "Wrong ephemeral should produce different key"
    );
}
