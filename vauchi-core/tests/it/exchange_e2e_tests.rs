// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-End Exchange Tests
//!
//! Tests the complete contact exchange flow from QR generation
//! through symmetric DH key agreement to card exchange.
//!
//! Feature file: features/contact_exchange.feature @e2e

use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, MockProximityVerifier, X3DHKeyPair,
};
use vauchi_core::{ContactCard, Identity};

// ============================================================
// Full Exchange Flow Tests
// Feature: contact_exchange.feature @e2e
// ============================================================

/// Test: Complete symmetric exchange produces matching shared keys
///
/// This verifies that:
/// 1. Alice and Bob both create QR sessions (no role distinction)
/// 2. Both display QR codes with fresh ephemerals, scan each other's
/// 3. Both derive the SAME shared key via symmetric DH
/// 4. Messages encrypted by one can be decrypted by the other
// @scenario: contact_exchange :: Exchange creates mutual keys
// @scenario: contact_exchange :: Successful QR code exchange with proximity
#[test]
fn test_full_exchange_produces_matching_shared_keys() {
    use vauchi_core::crypto::{decrypt, encrypt};

    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    // Both create symmetric QR sessions (no initiator/responder)
    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Step 1: Both start QR → DisplayingQr
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    // Step 2: Both scan each other's QR → PeerScanned
    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Step 3: Both confirm the other scanned → AwaitingKeyAgreement
    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Step 4: Both perform symmetric DH key agreement
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Step 5: Complete exchange
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card.clone()))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card.clone()))
        .unwrap();

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
        ExchangeState::Complete { contact } => contact
            .shared_key()
            .expect("Alice's completed contact should have a shared key"),
        _ => panic!("Alice should be complete"),
    };
    let bob_shared_key = match bob_session.state() {
        ExchangeState::Complete { contact } => contact
            .shared_key()
            .expect("Bob's completed contact should have a shared key"),
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
    let ciphertext = encrypt(alice_shared_key, message).unwrap();
    let decrypted = decrypt(bob_shared_key, &ciphertext).unwrap();
    assert_eq!(decrypted, message, "Bob should decrypt Alice's message");

    let message2 = b"Hello from Bob!";
    let ciphertext2 = encrypt(bob_shared_key, message2).unwrap();
    let decrypted2 = decrypt(alice_shared_key, &ciphertext2).unwrap();
    assert_eq!(decrypted2, message2, "Alice should decrypt Bob's message");
}

/// Test: Symmetric DH produces matching keys
///
/// Both sides have fresh ephemeral X25519 keys. DH is commutative:
/// DH(alice_secret, bob_public) == DH(bob_secret, alice_public)
// @scenario: contact_exchange :: X3DH key agreement during exchange
#[test]
fn test_symmetric_dh_produces_matching_keys() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let alice_shared = alice_keys.diffie_hellman(bob_keys.public_key()).unwrap();
    let bob_shared = bob_keys.diffie_hellman(alice_keys.public_key()).unwrap();

    assert_eq!(
        alice_shared, bob_shared,
        "Symmetric DH should produce identical shared secrets"
    );
}

/// Test: QR code contains ephemeral exchange key (not identity key)
// @scenario: contact_exchange :: Mutual QR uses fresh ephemeral keys for forward secrecy
#[test]
fn test_qr_contains_ephemeral_exchange_key() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(&identity, &ephemeral);

    // QR should have the ephemeral exchange key, not the identity's static key
    let exchange_key = qr.exchange_key();
    let signing_key = qr.public_key();

    assert_ne!(
        exchange_key, signing_key,
        "Exchange key should be different from signing key"
    );

    // Exchange key should match the ephemeral, NOT identity's exchange key
    assert_eq!(
        exchange_key,
        ephemeral.public_key(),
        "Exchange key should be the provided ephemeral key"
    );
    assert_ne!(
        exchange_key,
        &identity.exchange_public_key()[..32],
        "Exchange key should NOT be the identity's static exchange key"
    );
}

// ============================================================
// State Machine Transition Tests
// ============================================================

/// Test: StartQR transitions from Idle to DisplayingQr
// @scenario: contact_exchange :: Generate exchange QR code
#[test]
fn test_start_qr_transitions_to_displaying() {
    let identity = Identity::create("Alice", 0);
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    assert!(matches!(session.state(), ExchangeState::Idle));

    session.apply(ExchangeEvent::StartQR).unwrap();

    assert!(
        matches!(session.state(), ExchangeState::DisplayingQr { .. }),
        "Should transition to DisplayingQr"
    );

    let qr = session.qr().expect("Should have QR in DisplayingQr");
    assert!(qr.verify_signature());
    assert!(!qr.is_expired());
}

/// Test: ProcessQR requires DisplayingQr state (must call StartQR first)
// @scenario: contact_exchange :: QR code exchange blocked without proximity
#[test]
fn test_process_qr_requires_displaying_qr_state() {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Bob generates a QR
    let bob_ephemeral = X3DHKeyPair::generate();
    let bob_qr = ExchangeQR::generate(&bob_identity, &bob_ephemeral);

    // Try to process QR from Idle (without calling StartQR first)
    let result = alice_session.apply(ExchangeEvent::ProcessQR(bob_qr));
    assert!(
        result.is_err(),
        "Should reject ProcessQR when not in DisplayingQr state"
    );
}

/// Test: TheyScannedOurQR requires PeerScanned state
// @scenario: contact_exchange :: QR code exchange blocked without proximity
#[test]
fn test_they_scanned_requires_peer_scanned_state() {
    let identity = Identity::create("Alice", 0);
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    session.apply(ExchangeEvent::StartQR).unwrap();

    // Try TheyScannedOurQR without scanning their QR first
    let result = session.apply(ExchangeEvent::TheyScannedOurQR);
    assert!(result.is_err(), "Should fail when not in PeerScanned state");
}

/// Test: Complete state transition sequence
// @scenario: contact_exchange :: Mutual QR exchange with bidirectional scanning
#[test]
fn test_complete_state_transition_sequence() {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // Idle
    assert!(matches!(alice_session.state(), ExchangeState::Idle));
    assert!(matches!(bob_session.state(), ExchangeState::Idle));

    // StartQR → DisplayingQr
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    assert!(matches!(
        alice_session.state(),
        ExchangeState::DisplayingQr { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::DisplayingQr { .. }
    ));

    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    // ProcessQR → PeerScanned
    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    assert!(matches!(
        alice_session.state(),
        ExchangeState::PeerScanned { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::PeerScanned { .. }
    ));

    // TheyScannedOurQR → AwaitingKeyAgreement
    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));

    // PerformKeyAgreement → AwaitingCardExchange
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingCardExchange { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingCardExchange { .. }
    ));

    // CompleteExchange → Complete
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();
    assert!(matches!(
        alice_session.state(),
        ExchangeState::Complete { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::Complete { .. }
    ));
}

// ============================================================
// Edge Cases
// ============================================================

/// Test: Wrong DH key produces different shared secret
// @scenario: security :: Man-in-the-middle detection during exchange
#[test]
fn test_wrong_dh_key_produces_different_secret() {
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();
    let eve_keys = X3DHKeyPair::generate();

    // Alice computes shared with Bob
    let alice_shared = alice_keys.diffie_hellman(bob_keys.public_key()).unwrap();

    // Alice computes shared with Eve (wrong party)
    let alice_eve_shared = alice_keys.diffie_hellman(eve_keys.public_key()).unwrap();

    // Keys should NOT match
    assert_ne!(
        alice_shared, alice_eve_shared,
        "DH with wrong party should produce different key"
    );
}

/// Test: Self-exchange is rejected
// @scenario: contact_exchange :: Cannot exchange with yourself
#[test]
fn test_self_exchange_rejected() {
    let alice_identity = Identity::create("Alice", 0);

    // Generate own QR before moving identity into session
    let own_ephemeral = X3DHKeyPair::generate();
    let own_qr = ExchangeQR::generate(&alice_identity, &own_ephemeral);

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(
        alice_identity,
        alice_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    session.apply(ExchangeEvent::StartQR).unwrap();

    let result = session.apply(ExchangeEvent::ProcessQR(own_qr));
    assert!(
        matches!(
            result,
            Err(vauchi_core::exchange::ExchangeError::SelfExchange)
        ),
        "Should reject self-exchange"
    );
}

/// Test: Expired QR is rejected
// @scenario: contact_exchange :: QR code expiration
// @scenario: contact_exchange :: Mutual QR rejects expired peer QR code
#[test]
fn test_expired_qr_rejected() {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    alice_session.apply(ExchangeEvent::StartQR).unwrap();

    // Bob's expired QR (6 minutes ago)
    let bob_ephemeral = X3DHKeyPair::generate();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expired_qr = ExchangeQR::generate_with_timestamp(&bob_identity, &bob_ephemeral, now - 360);

    let result = alice_session.apply(ExchangeEvent::ProcessQR(expired_qr));
    assert!(
        matches!(result, Err(vauchi_core::exchange::ExchangeError::QRExpired)),
        "Should reject expired QR"
    );
}

/// Test: Fresh ephemeral is used, not identity's static exchange key
// @scenario: contact_exchange :: Mutual QR uses fresh ephemeral keys for forward secrecy
#[test]
fn test_session_uses_fresh_ephemeral_not_identity_key() {
    let alice_identity = Identity::create("Alice", 0);

    // Capture identity exchange key before moving identity into session
    let identity_exchange: [u8; 32] = alice_identity
        .exchange_public_key()
        .try_into()
        .expect("32 bytes");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_qr(
        alice_identity,
        alice_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // The session's exchange key should NOT match identity's exchange key
    let session_exchange = session.our_exchange_public_key();

    assert_ne!(
        session_exchange, &identity_exchange,
        "QR session should use fresh ephemeral, not identity's exchange key"
    );
}

/// Test: Contact names are correct after exchange
// @scenario: contact_exchange :: Successful QR code exchange with proximity
#[test]
fn test_contact_names_correct_after_exchange() {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // Run full exchange
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

    // Verify contact names
    match alice_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(contact.display_name(), "Bob");
        }
        _ => panic!("Alice should be Complete"),
    }
    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(contact.display_name(), "Alice");
        }
        _ => panic!("Bob should be Complete"),
    }
}

/// Test: Each session gets a unique ephemeral key
///
/// Even with the same identity, two sessions should have different
/// ephemeral keys because each generates a fresh X25519 keypair.
// @scenario: contact_exchange :: Mutual QR uses fresh ephemeral keys for forward secrecy
#[test]
fn test_each_session_gets_unique_ephemeral() {
    let identity1 = Identity::create("Alice", 0);
    let identity2 = Identity::create("Alice", 0);
    let card = ContactCard::new("Alice");

    let session1 = ExchangeSession::new_qr(
        identity1,
        card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let session2 = ExchangeSession::new_qr(
        identity2,
        card,
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    assert_ne!(
        session1.our_exchange_public_key(),
        session2.our_exchange_public_key(),
        "Each session should have a unique ephemeral key"
    );
}
