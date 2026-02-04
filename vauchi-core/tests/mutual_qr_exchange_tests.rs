// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Mutual QR Exchange (Feature A)
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

#[test]
fn test_qr_generate_with_ephemeral() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_ephemeral(&identity, &ephemeral);

    // Identity key should match
    assert_eq!(qr.public_key(), identity.signing_public_key());

    // Exchange key should be the ephemeral, NOT the identity's exchange key
    assert_eq!(qr.exchange_key(), ephemeral.public_key());
    assert_ne!(
        qr.exchange_key(),
        identity.exchange_public_key(),
        "Ephemeral QR should NOT use identity's exchange key"
    );

    // Should be valid
    assert!(qr.verify_signature());
    assert!(!qr.is_expired());
}

#[test]
fn test_qr_ephemeral_changes_each_call() {
    let identity = Identity::create("Alice");

    let eph1 = X3DHKeyPair::generate();
    let eph2 = X3DHKeyPair::generate();

    let qr1 = ExchangeQR::generate_with_ephemeral(&identity, &eph1);
    let qr2 = ExchangeQR::generate_with_ephemeral(&identity, &eph2);

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

#[test]
fn test_qr_ephemeral_roundtrip_via_data_string() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_ephemeral(&identity, &ephemeral);
    let data = qr.to_data_string();
    let parsed = ExchangeQR::from_data_string(&data).expect("Should parse");

    assert_eq!(parsed.public_key(), qr.public_key());
    assert_eq!(parsed.exchange_key(), qr.exchange_key());
    assert_eq!(parsed.exchange_key(), ephemeral.public_key());
    assert!(parsed.verify_signature());
}

// ============================================================
// Mutual QR state machine transitions
// ============================================================

#[test]
fn test_mutual_start_generates_qr() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_mutual_qr(identity, card, proximity);

    assert!(matches!(session.state(), ExchangeState::Idle));
    assert_eq!(session.transport(), ExchangeTransport::QrMutual);

    session.apply(ExchangeEvent::StartMutualQR).unwrap();

    assert!(
        matches!(
            session.state(),
            ExchangeState::MutualAwaitingTheirScan { .. }
        ),
        "Should transition to MutualAwaitingTheirScan"
    );

    let qr = session
        .qr()
        .expect("Should have QR in MutualAwaitingTheirScan");
    assert!(qr.verify_signature());
    assert!(!qr.is_expired());
}

#[test]
fn test_mutual_start_rejects_wrong_transport() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    // One-way QR session
    let mut session = ExchangeSession::new_initiator(identity, card, proximity);

    let result = session.apply(ExchangeEvent::StartMutualQR);
    assert!(
        result.is_err(),
        "Should reject StartMutualQR on QrOneWay transport"
    );
}

#[test]
fn test_mutual_scan_their_qr_transitions() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_mutual_qr(alice_identity, alice_card, proximity);

    // Alice starts mutual QR
    alice_session.apply(ExchangeEvent::StartMutualQR).unwrap();

    // Bob also creates a QR (simulating his side)
    let bob_ephemeral = X3DHKeyPair::generate();
    let bob_qr = ExchangeQR::generate_with_ephemeral(&bob_identity, &bob_ephemeral);

    // Alice scans Bob's QR
    alice_session
        .apply(ExchangeEvent::ScannedTheirQR(bob_qr))
        .unwrap();

    assert!(
        matches!(alice_session.state(), ExchangeState::MutualVerified { .. }),
        "Should transition to MutualVerified"
    );

    // QR should still be accessible (for Bob to scan)
    assert!(alice_session.qr().is_some());
}

#[test]
fn test_mutual_scan_rejects_expired() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_mutual_qr(alice_identity, alice_card, proximity);
    alice_session.apply(ExchangeEvent::StartMutualQR).unwrap();

    // Bob's expired QR (6 minutes ago)
    let bob_ephemeral = X3DHKeyPair::generate();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expired_qr =
        ExchangeQR::generate_with_ephemeral_and_timestamp(&bob_identity, &bob_ephemeral, now - 360);

    let result = alice_session.apply(ExchangeEvent::ScannedTheirQR(expired_qr));
    assert!(
        matches!(result, Err(vauchi_core::exchange::ExchangeError::QRExpired)),
        "Should reject expired QR"
    );
}

#[test]
fn test_mutual_scan_rejects_self_exchange() {
    let alice_identity = Identity::create("Alice");

    // Generate own QR before moving identity into session
    let own_ephemeral = X3DHKeyPair::generate();
    let own_qr = ExchangeQR::generate_with_ephemeral(&alice_identity, &own_ephemeral);

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_mutual_qr(alice_identity, alice_card, proximity);
    session.apply(ExchangeEvent::StartMutualQR).unwrap();

    let result = session.apply(ExchangeEvent::ScannedTheirQR(own_qr));
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

#[test]
fn test_mutual_key_agreement_symmetric() {
    // Both sides use fresh ephemerals, so DH should be symmetric:
    // Alice: DH(alice_secret, bob_public) == Bob: DH(bob_secret, alice_public)
    let alice_keys = X3DHKeyPair::generate();
    let bob_keys = X3DHKeyPair::generate();

    let alice_shared = alice_keys.diffie_hellman(bob_keys.public_key());
    let bob_shared = bob_keys.diffie_hellman(alice_keys.public_key());

    assert_eq!(
        alice_shared, bob_shared,
        "Symmetric DH should produce identical shared secrets"
    );
}

#[test]
fn test_mutual_key_differs_from_oneway() {
    // Mutual QR uses fresh ephemerals; one-way QR uses identity's X3DH key.
    // With different keys, the shared secrets should differ.
    let alice_identity = Identity::create("Alice");
    let _bob_identity = Identity::create("Bob");

    // One-way: Bob initiates X3DH with Alice's identity exchange key
    let alice_identity_x3dh = alice_identity.x3dh_keypair();
    let bob_oneway = X3DHKeyPair::generate();
    let oneway_shared = bob_oneway.diffie_hellman(alice_identity_x3dh.public_key());

    // Mutual: both use fresh ephemerals
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_ephemeral = X3DHKeyPair::generate();
    let mutual_shared = alice_ephemeral.diffie_hellman(bob_ephemeral.public_key());

    // Different keys → different shared secrets
    // (with overwhelming probability, since they use different key material)
    assert_ne!(
        oneway_shared, mutual_shared,
        "Mutual and one-way should produce different shared secrets (different key material)"
    );

    // But the identity exchange key is deterministic for one-way
    assert_eq!(
        alice_identity_x3dh.public_key(),
        &alice_identity.exchange_public_key()[..32],
        "One-way QR uses identity's exchange key"
    );

    // Mutual ephemeral should NOT match identity key
    assert_ne!(
        alice_ephemeral.public_key(),
        alice_identity_x3dh.public_key(),
        "Mutual ephemeral should be different from identity exchange key"
    );
}

// ============================================================
// Full mutual QR exchange lifecycle
// ============================================================

#[test]
fn test_mutual_full_exchange() {
    use vauchi_core::crypto::{decrypt, encrypt};

    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    // Both create mutual QR sessions
    let alice_proximity = MockProximityVerifier::success();
    let bob_proximity = MockProximityVerifier::success();

    let mut alice_session =
        ExchangeSession::new_mutual_qr(alice_identity, alice_card.clone(), alice_proximity);
    let mut bob_session =
        ExchangeSession::new_mutual_qr(bob_identity, bob_card.clone(), bob_proximity);

    // Step 1: Both start mutual QR — each generates a QR with fresh ephemeral
    alice_session.apply(ExchangeEvent::StartMutualQR).unwrap();
    bob_session.apply(ExchangeEvent::StartMutualQR).unwrap();

    // Get each other's QR codes
    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    // Step 2: Both scan each other's QR
    alice_session
        .apply(ExchangeEvent::ScannedTheirQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ScannedTheirQR(alice_qr))
        .unwrap();

    // Both in MutualVerified state
    assert!(matches!(
        alice_session.state(),
        ExchangeState::MutualVerified { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::MutualVerified { .. }
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
    let msg = b"Hello from Alice via mutual QR!";
    let ct = encrypt(&alice_shared, msg).unwrap();
    let pt = decrypt(&bob_shared, &ct).unwrap();
    assert_eq!(pt, msg);

    let msg2 = b"Hello from Bob via mutual QR!";
    let ct2 = encrypt(&bob_shared, msg2).unwrap();
    let pt2 = decrypt(&alice_shared, &ct2).unwrap();
    assert_eq!(pt2, msg2);
}

#[test]
fn test_mutual_uses_fresh_ephemeral_not_identity() {
    let alice_identity = Identity::create("Alice");

    // Capture identity exchange key before moving identity into session
    let identity_exchange: [u8; 32] = alice_identity
        .exchange_public_key()
        .try_into()
        .expect("32 bytes");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_mutual_qr(alice_identity, alice_card, proximity);

    // The session's exchange key should NOT match identity's exchange key
    let session_exchange = session.our_exchange_public_key();

    assert_ne!(
        session_exchange, &identity_exchange,
        "Mutual QR should use fresh ephemeral, not identity's exchange key"
    );
}

#[test]
fn test_mutual_they_scanned_requires_verified_state() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_mutual_qr(identity, card, proximity);
    session.apply(ExchangeEvent::StartMutualQR).unwrap();

    // Try TheyScannedOurQR without scanning their QR first
    let result = session.apply(ExchangeEvent::TheyScannedOurQR);
    assert!(
        result.is_err(),
        "Should fail when not in MutualVerified state"
    );
}

#[test]
fn test_mutual_qr_contact_names_correct() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_mutual_qr(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
    );
    let mut bob_session = ExchangeSession::new_mutual_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
    );

    alice_session.apply(ExchangeEvent::StartMutualQR).unwrap();
    bob_session.apply(ExchangeEvent::StartMutualQR).unwrap();

    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    alice_session
        .apply(ExchangeEvent::ScannedTheirQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ScannedTheirQR(alice_qr))
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
