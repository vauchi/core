//! Tests for exchange::session
//! Extracted from session.rs

use webbook_core::exchange::MockProximityVerifier;
use webbook_core::exchange::*;
use webbook_core::*;

#[test]
fn test_initiator_generates_qr() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_initiator(identity, card, proximity);

    assert!(matches!(session.state(), ExchangeState::Idle));

    let qr = session.generate_qr().unwrap();
    assert!(!qr.is_expired());

    assert!(matches!(
        session.state(),
        ExchangeState::AwaitingScan { .. }
    ));
}

#[test]
fn test_responder_processes_qr() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    // Alice generates QR
    let alice_qr = ExchangeQR::generate(&alice_identity);

    // Bob processes it
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

    bob_session.process_scanned_qr(alice_qr).unwrap();

    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingProximity { .. }
    ));
}

#[test]
fn test_expired_qr_rejected() {
    let identity = Identity::create("Alice");
    let old_qr = ExchangeQR::generate_with_timestamp(
        &identity,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 360, // 6 minutes ago
    );

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

    let result = session.process_scanned_qr(old_qr);
    assert!(matches!(result, Err(ExchangeError::QRExpired)));
}

#[test]
fn test_session_timeout() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_initiator(identity, card, proximity);

    // Fresh session should not be timed out
    assert!(!session.is_timed_out());
}

#[test]
fn test_session_resume() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_initiator(identity, card, proximity);

    // Not interrupted yet
    assert!(!session.can_resume());

    // Mark as interrupted
    session.mark_interrupted();
    assert!(session.can_resume());
}

#[test]
fn test_detect_duplicate_contact() {
    use webbook_core::crypto::SymmetricKey;

    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    // Create an existing contact with Alice's public key
    let alice_card = ContactCard::new("Alice");
    let existing_alice = Contact::from_exchange(
        *alice_identity.signing_public_key(),
        alice_card.clone(),
        SymmetricKey::generate(),
    );

    let contacts = vec![existing_alice];

    // Bob scans Alice's QR
    let alice_qr = ExchangeQR::generate(&alice_identity);
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

    bob_session.process_scanned_qr(alice_qr).unwrap();

    // Should detect Alice as duplicate
    let duplicate = bob_session.check_duplicate(&contacts);
    assert!(duplicate.is_some());
    assert_eq!(duplicate.unwrap().display_name(), "Alice");
}

#[test]
fn test_no_duplicate_for_new_contact() {
    use webbook_core::crypto::SymmetricKey;

    let alice_identity = Identity::create("Alice");
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
    let alice_qr = ExchangeQR::generate(&alice_identity);
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

    bob_session.process_scanned_qr(alice_qr).unwrap();

    // Should NOT detect a duplicate
    let duplicate = bob_session.check_duplicate(&contacts);
    assert!(duplicate.is_none());
}

#[test]
fn test_duplicate_action_variants() {
    // Just verify the enum variants exist and can be compared
    assert_eq!(DuplicateAction::Update, DuplicateAction::Update);
    assert_ne!(DuplicateAction::Update, DuplicateAction::Keep);
    assert_ne!(DuplicateAction::Keep, DuplicateAction::Cancel);
}
