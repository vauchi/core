// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for exchange::session
//! Covers the QR exchange state machine: new_qr(), StartQR, ProcessQR,
//! TheyScannedOurQR, PerformKeyAgreement, CompleteExchange flow.

use vauchi_core::exchange::*;
use vauchi_core::*;

// @scenario: contact_exchange.feature:Default QR exchange uses mutual flow
#[test]
fn test_new_qr_starts_idle() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_qr(identity, card, proximity);

    assert!(matches!(session.state(), ExchangeState::Idle));
    assert_eq!(session.transport(), ExchangeTransport::Qr);
}

// @scenario: contact_exchange.feature:Generate exchange QR code
#[test]
fn test_start_qr_transitions_to_displaying_qr() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(identity, card, proximity);

    session.apply(ExchangeEvent::StartQR).unwrap();
    let qr = session.qr().expect("Expected QR code");
    assert!(!qr.is_expired());

    assert!(matches!(
        session.state(),
        ExchangeState::DisplayingQr { .. }
    ));
}

// @scenario: contact_exchange.feature:Mutual QR exchange with bidirectional scanning
#[test]
fn test_process_qr_transitions_to_peer_scanned() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    // Alice generates a QR with her identity and ephemeral key
    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    // Bob creates a session, starts displaying his QR, then scans Alice's
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    assert!(matches!(
        bob_session.state(),
        ExchangeState::PeerScanned { .. }
    ));
}

// @scenario: contact_exchange.feature:Mutual QR exchange with bidirectional scanning
#[test]
fn test_process_qr_requires_displaying_qr_state() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    // Bob tries to process QR without first starting his own QR display
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    let result = bob_session.apply(ExchangeEvent::ProcessQR(alice_qr));
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// @scenario: contact_exchange.feature:Mutual QR rejects expired peer QR code
#[test]
fn test_expired_qr_rejected() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let old_qr = ExchangeQR::generate_with_timestamp(
        &alice_identity,
        &alice_ephemeral,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 360, // 6 minutes ago
    );

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    // Must be in DisplayingQr state before processing
    session.apply(ExchangeEvent::StartQR).unwrap();

    let result = session.apply(ExchangeEvent::ProcessQR(old_qr));
    assert!(matches!(result, Err(ExchangeError::QRExpired)));
}

// @scenario: contact_exchange.feature:Mutual QR exchange with bidirectional scanning
#[test]
fn test_they_scanned_our_qr_transitions_to_awaiting_key_agreement() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    // Bob: Idle -> DisplayingQr -> PeerScanned -> AwaitingKeyAgreement
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));
}

// @scenario: contact_exchange.feature:Successful QR code exchange with proximity
// @scenario: contact_exchange.feature:Exchange creates mutual keys
#[test]
fn test_full_qr_exchange_flow() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    // Full flow: Idle -> DisplayingQr -> PeerScanned -> AwaitingKeyAgreement
    //            -> AwaitingCardExchange -> Complete
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingCardExchange { .. }
    ));

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    assert!(matches!(
        bob_session.state(),
        ExchangeState::Complete { .. }
    ));
}

// @scenario: contact_exchange.feature:Exchange session timeout
#[test]
fn test_session_timeout() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_qr(identity, card, proximity);

    // Fresh session should not be timed out
    assert!(!session.is_timed_out());
}

// @scenario: contact_exchange.feature:Exchange session timeout
#[test]
fn test_session_resume() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(identity, card, proximity);

    // Not interrupted yet
    assert!(!session.can_resume());

    // Mark as interrupted
    session.mark_interrupted();
    assert!(session.can_resume());
}

// @scenario: contact_exchange.feature:Exchange with existing contact shows update option
// @scenario: contact_exchange.feature:Update existing contact via exchange
#[test]
fn test_detect_duplicate_contact() {
    use vauchi_core::crypto::SymmetricKey;

    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    // Create an existing contact with Alice's public key
    let alice_card = ContactCard::new("Alice");
    let existing_alice = Contact::from_exchange(
        *alice_identity.signing_public_key(),
        alice_card.clone(),
        SymmetricKey::generate(),
    );

    let contacts = vec![existing_alice];

    // Alice generates QR with her identity and ephemeral key
    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    // Bob must be in DisplayingQr state first, then scan Alice's QR
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Should detect Alice as duplicate (session is in PeerScanned state)
    let duplicate = bob_session.check_duplicate(&contacts);
    assert!(duplicate.is_some());
    assert_eq!(duplicate.unwrap().display_name(), "Alice");
}

// @scenario: contact_exchange.feature:Exchange with existing contact shows update option
#[test]
fn test_no_duplicate_for_new_contact() {
    use vauchi_core::crypto::SymmetricKey;

    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");
    let charlie_identity = Identity::create("Charlie");

    // Create an existing contact with Charlie's public key
    let charlie_card = ContactCard::new("Charlie");
    let existing_charlie = Contact::from_exchange(
        *charlie_identity.signing_public_key(),
        charlie_card,
        SymmetricKey::generate(),
    );

    let contacts = vec![existing_charlie];

    // Bob scans Alice's QR (Alice is not in contacts)
    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Should NOT detect a duplicate
    let duplicate = bob_session.check_duplicate(&contacts);
    assert!(duplicate.is_none());
}

// @scenario: contact_exchange.feature:Update existing contact via exchange
// @scenario: contact_exchange.feature:Keep existing contact without update
#[test]
fn test_duplicate_action_variants() {
    // Just verify the enum variants exist and can be compared
    assert_eq!(DuplicateAction::Update, DuplicateAction::Update);
    assert_ne!(DuplicateAction::Update, DuplicateAction::Keep);
    assert_ne!(DuplicateAction::Keep, DuplicateAction::Cancel);
}
