// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for exchange::session
//! Covers the QR exchange state machine: new_qr(), StartQR, ProcessQR,
//! TheyScannedOurQR, PerformKeyAgreement, CompleteExchange flow.

use vauchi_core::exchange::*;
use vauchi_core::*;

// @scenario: contact_exchange :: Default QR exchange uses mutual flow
#[test]
fn test_new_qr_starts_idle() {
    let identity = Identity::create("Alice", 0);
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    assert!(matches!(session.state(), ExchangeState::Idle));
    assert_eq!(session.transport(), ExchangeTransport::Qr);
}

// @scenario: contact_exchange :: Generate exchange QR code
#[test]
fn test_start_qr_transitions_to_displaying_qr() {
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
    let qr = session.qr().expect("Expected QR code");
    assert!(!qr.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()));

    assert!(matches!(
        session.state(),
        ExchangeState::DisplayingQr { .. }
    ));
}

// @scenario: contact_exchange :: Mutual QR exchange with bidirectional scanning
#[test]
fn test_process_qr_transitions_to_peer_scanned() {
    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob", 0);

    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    assert!(matches!(
        bob_session.state(),
        ExchangeState::PeerScanned { .. }
    ));
}

// @scenario: contact_exchange :: Mutual QR exchange with bidirectional scanning
#[test]
fn test_process_qr_requires_displaying_qr_state() {
    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob", 0);

    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    let result = bob_session.apply(ExchangeEvent::ProcessQR(alice_qr));
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// @scenario: contact_exchange :: Mutual QR rejects expired peer QR code
#[test]
fn test_expired_qr_rejected() {
    let alice_identity = Identity::create("Alice", 0);
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

    let bob_identity = Identity::create("Bob", 0);
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Must be in DisplayingQr state before processing
    session.apply(ExchangeEvent::StartQR).unwrap();

    let result = session.apply(ExchangeEvent::ProcessQR(old_qr));
    assert!(matches!(result, Err(ExchangeError::QRExpired)));
}

// @scenario: contact_exchange :: Mutual QR exchange with bidirectional scanning
#[test]
fn test_they_scanned_our_qr_transitions_to_awaiting_key_agreement() {
    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob", 0);

    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

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

// @scenario: contact_exchange :: Successful QR code exchange with proximity
// @scenario: contact_exchange :: Exchange creates mutual keys
#[test]
fn test_full_qr_exchange_flow() {
    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob", 0);

    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

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

// @scenario: contact_exchange :: Exchange session timeout
#[test]
fn test_session_timeout() {
    let identity = Identity::create("Alice", 0);
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    assert!(!session.is_timed_out());
}

// @scenario: contact_exchange :: Exchange session timeout
#[test]
fn test_session_resume() {
    let identity = Identity::create("Alice", 0);
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    assert!(!session.can_resume());

    session.mark_interrupted();
    assert!(session.can_resume());
}

// @scenario: contact_exchange :: Exchange with existing contact shows update option
// @scenario: contact_exchange :: Update existing contact via exchange
#[test]
fn test_detect_duplicate_contact() {
    use vauchi_core::crypto::SymmetricKey;

    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let existing_alice = Contact::from_exchange(
        *alice_identity.signing_public_key(),
        alice_card.clone(),
        SymmetricKey::generate(),
        0,
    );

    let contacts = vec![existing_alice];

    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Bob must be in DisplayingQr state first, then scan Alice's QR
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Should detect Alice as duplicate (session is in PeerScanned state)
    let duplicate = bob_session.check_duplicate(&contacts);
    duplicate.expect("expected Some");
    assert_eq!(duplicate.unwrap().display_name(), "Alice");
}

// @scenario: contact_exchange :: Exchange with existing contact shows update option
#[test]
fn test_no_duplicate_for_new_contact() {
    use vauchi_core::crypto::SymmetricKey;

    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob", 0);
    let charlie_identity = Identity::create("Charlie", 0);

    let charlie_card = ContactCard::new("Charlie");
    let existing_charlie = Contact::from_exchange(
        *charlie_identity.signing_public_key(),
        charlie_card,
        SymmetricKey::generate(),
        0,
    );

    let contacts = vec![existing_charlie];

    // Bob scans Alice's QR (Alice is not in contacts)
    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    let duplicate = bob_session.check_duplicate(&contacts);
    assert!(duplicate.is_none());
}

// @scenario: contact_exchange :: Update existing contact via exchange
// @scenario: contact_exchange :: Keep existing contact without update
#[test]
fn test_duplicate_action_variants() {
    // Just verify the enum variants exist and can be compared
    assert_eq!(DuplicateAction::Update, DuplicateAction::Update);
    assert_ne!(DuplicateAction::Update, DuplicateAction::Keep);
    assert_ne!(DuplicateAction::Keep, DuplicateAction::Cancel);
}
