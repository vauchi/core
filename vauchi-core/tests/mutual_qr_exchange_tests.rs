// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for QR Exchange
//!
//! Feature file: features/contact_exchange.feature @qr-mutual
//!
//! Both users display QR codes and scan each other's. Both sides use
//! fresh ephemeral X25519 keys for full forward secrecy.

use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, ExchangeTransport,
    MockProximityVerifier, X3DHKeyPair,
};
use vauchi_core::{ContactCard, Identity};

// ============================================================
// QR with ephemeral keys
// ============================================================

// @scenario: contact_exchange.feature:Mutual QR uses fresh ephemeral keys for forward secrecy
// @scenario: contact_exchange.feature:Generate exchange QR code
#[test]
fn test_qr_generate_with_ephemeral() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate(&identity, &ephemeral);

    // Identity key should match
    assert_eq!(qr.public_key(), identity.signing_public_key());

    // Exchange key should be the ephemeral, NOT the identity's exchange key
    assert_eq!(qr.exchange_key(), ephemeral.public_key());
    assert_ne!(
        qr.exchange_key(),
        identity.exchange_public_key(),
        "QR should NOT use identity's static exchange key"
    );

    // Should be valid
    assert!(qr.verify_signature());
    assert!(!qr.is_expired());
}

// @scenario: contact_exchange.feature:Mutual QR uses fresh ephemeral keys for forward secrecy
#[test]
fn test_qr_ephemeral_changes_each_call() {
    let identity = Identity::create("Alice");

    let eph1 = X3DHKeyPair::generate();
    let eph2 = X3DHKeyPair::generate();

    let qr1 = ExchangeQR::generate(&identity, &eph1);
    let qr2 = ExchangeQR::generate(&identity, &eph2);

    // Same identity key
    assert_eq!(qr1.public_key(), qr2.public_key());

    // Different exchange keys (different ephemerals)
    assert_ne!(
        qr1.exchange_key(),
        qr2.exchange_key(),
        "Each call with different ephemerals should produce different exchange keys"
    );

    // Both valid
    assert!(qr1.verify_signature());
    assert!(qr2.verify_signature());
}

// @scenario: contact_exchange.feature:Generate exchange QR code
#[test]
fn test_qr_ephemeral_roundtrip_via_data_string() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate(&identity, &ephemeral);
    let data = qr.to_data_string();
    let parsed = ExchangeQR::from_data_string(&data).expect("Should parse");

    assert_eq!(parsed.public_key(), qr.public_key());
    assert_eq!(parsed.exchange_key(), qr.exchange_key());
    assert_eq!(parsed.exchange_key(), ephemeral.public_key());
    assert!(parsed.verify_signature());
}

// ============================================================
// QR state machine transitions
// ============================================================

// @scenario: contact_exchange.feature:Default QR exchange uses mutual flow
#[test]
fn test_start_qr_generates_qr() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(identity, card, proximity);

    assert!(matches!(session.state(), ExchangeState::Idle));
    assert_eq!(session.transport(), ExchangeTransport::Qr);

    session.apply(ExchangeEvent::StartQR).unwrap();

    assert!(
        matches!(session.state(), ExchangeState::DisplayingQr { .. }),
        "Should transition to DisplayingQr"
    );

    let qr = session.qr().expect("Should have QR in DisplayingQr");
    assert!(qr.verify_signature());
    assert!(!qr.is_expired());
}

#[test]
fn test_start_qr_rejects_wrong_transport() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    // NFC session — StartQR should fail
    let mut session = ExchangeSession::new_nfc(identity, card, proximity);

    let result = session.apply(ExchangeEvent::StartQR);
    assert!(result.is_err(), "Should reject StartQR on NFC transport");
}

#[test]
fn test_scan_their_qr_transitions() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_qr(alice_identity, alice_card, proximity);

    // Alice starts QR
    alice_session.apply(ExchangeEvent::StartQR).unwrap();

    // Bob also creates a QR (simulating his side)
    let bob_ephemeral = X3DHKeyPair::generate();
    let bob_qr = ExchangeQR::generate(&bob_identity, &bob_ephemeral);

    // Alice scans Bob's QR
    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();

    assert!(
        matches!(alice_session.state(), ExchangeState::PeerScanned { .. }),
        "Should transition to PeerScanned"
    );

    // QR should still be accessible (for Bob to scan)
    assert!(alice_session.qr().is_some());
}

// @scenario: contact_exchange.feature:Mutual QR rejects expired peer QR code
#[test]
fn test_scan_rejects_expired() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_qr(alice_identity, alice_card, proximity);
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

// @scenario: contact_exchange.feature:Mutual QR prevents self-exchange
// @scenario: contact_exchange.feature:Cannot exchange with yourself
#[test]
fn test_scan_rejects_self_exchange() {
    let alice_identity = Identity::create("Alice");

    // Generate own QR before moving identity into session
    let own_ephemeral = X3DHKeyPair::generate();
    let own_qr = ExchangeQR::generate(&alice_identity, &own_ephemeral);

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(alice_identity, alice_card, proximity);
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

// ============================================================
// Symmetric DH key agreement
// ============================================================

// @scenario: contact_exchange.feature:Successful QR code exchange with proximity
// @scenario: contact_exchange.feature:X3DH key agreement during exchange
#[test]
fn test_key_agreement_symmetric() {
    // Both sides use fresh ephemerals, so DH should be symmetric:
    // Alice: DH(alice_secret, bob_public) == Bob: DH(bob_secret, alice_public)
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let alice_shared = alice_keys.diffie_hellman(bob_keys.public_key()).unwrap();
    let bob_shared = bob_keys.diffie_hellman(alice_keys.public_key()).unwrap();

    assert_eq!(
        alice_shared, bob_shared,
        "Symmetric DH should produce identical shared secrets"
    );
}

// ============================================================
// Full QR exchange lifecycle
// ============================================================

// @scenario: contact_exchange.feature:Successful QR code exchange with proximity
// @scenario: contact_exchange.feature:Mutual QR exchange with bidirectional scanning
// @scenario: contact_exchange.feature:Exchange creates mutual keys
#[test]
fn test_full_qr_exchange() {
    use vauchi_core::crypto::{decrypt, encrypt};

    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let alice_proximity = MockProximityVerifier::success();
    let bob_proximity = MockProximityVerifier::success();

    let mut alice_session =
        ExchangeSession::new_qr(alice_identity, alice_card.clone(), alice_proximity);
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card.clone(), bob_proximity);

    // Step 1: Both start QR — each generates a QR with fresh ephemeral
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Get each other's QR codes
    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    // Step 2: Both scan each other's QR
    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Both in PeerScanned state
    assert!(matches!(
        alice_session.state(),
        ExchangeState::PeerScanned { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::PeerScanned { .. }
    ));

    // Step 3: Both confirm the other scanned (transition to key agreement)
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

    // Step 4: Both perform key agreement (symmetric DH)
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

    // Step 5: Exchange cards
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    // Both complete
    assert!(matches!(
        alice_session.state(),
        ExchangeState::Complete { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::Complete { .. }
    ));

    // Verify matching shared keys
    let alice_shared = match alice_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("Expected Complete"),
    };
    let bob_shared = match bob_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().clone(),
        _ => panic!("Expected Complete"),
    };

    assert_eq!(
        alice_shared.as_bytes(),
        bob_shared.as_bytes(),
        "Both sides should derive the same shared key"
    );

    // Verify encryption works bidirectionally
    let msg = b"Hello from Alice via QR!";
    let ct = encrypt(&alice_shared, msg).unwrap();
    let pt = decrypt(&bob_shared, &ct).unwrap();
    assert_eq!(pt, msg);

    let msg2 = b"Hello from Bob via QR!";
    let ct2 = encrypt(&bob_shared, msg2).unwrap();
    let pt2 = decrypt(&alice_shared, &ct2).unwrap();
    assert_eq!(pt2, msg2);
}

// @scenario: contact_exchange.feature:Mutual QR uses fresh ephemeral keys for forward secrecy
#[test]
fn test_qr_uses_fresh_ephemeral_not_identity() {
    let alice_identity = Identity::create("Alice");

    // Capture identity exchange key before moving identity into session
    let identity_exchange: [u8; 32] = alice_identity
        .exchange_public_key()
        .try_into()
        .expect("32 bytes");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_qr(alice_identity, alice_card, proximity);

    // The session's exchange key should NOT match identity's exchange key
    let session_exchange = session.our_exchange_public_key();

    assert_ne!(
        session_exchange, &identity_exchange,
        "QR should use fresh ephemeral, not identity's exchange key"
    );
}

#[test]
fn test_they_scanned_requires_peer_scanned_state() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(identity, card, proximity);
    session.apply(ExchangeEvent::StartQR).unwrap();

    // Try TheyScannedOurQR without scanning their QR first
    let result = session.apply(ExchangeEvent::TheyScannedOurQR);
    assert!(result.is_err(), "Should fail when not in PeerScanned state");
}

#[test]
fn test_qr_contact_names_correct() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
    );

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
