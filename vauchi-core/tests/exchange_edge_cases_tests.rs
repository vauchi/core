// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Edge Cases Tests
//!
//! Tests for edge cases in contact exchange flow.
//! Based on: features/contact_exchange.feature
//!
//! Updated for the mutual QR-only API: no roles, no proximity step.
//! The flow is: Idle → StartQR → DisplayingQr → ProcessQR → PeerScanned
//!   → TheyScannedOurQR → AwaitingKeyAgreement → PerformKeyAgreement
//!   → AwaitingCardExchange → CompleteExchange → Complete.

use vauchi_core::exchange::{
    ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState,
    MockProximityVerifier, X3DHKeyPair,
};
use vauchi_core::identity::Identity;
use vauchi_core::ContactCard;

/// Helper to create a mock proximity verifier.
///
/// Proximity verification is still used for audio/BLE transports but no
/// longer drives any state transition in the QR flow. We pass a mock
/// verifier because `ExchangeSession` is generic over `ProximityVerifier`.
fn mock_proximity() -> MockProximityVerifier {
    MockProximityVerifier::success()
}

/// Helper: advance a QR session through the full happy-path up to
/// (but not including) `CompleteExchange`. Returns both sessions in
/// `AwaitingCardExchange` state.
///
/// Creates fresh identities internally since `Identity` is not `Clone`.
fn advance_to_card_exchange() -> (
    ExchangeSession<MockProximityVerifier>,
    ExchangeSession<MockProximityVerifier>,
) {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let mut alice =
        ExchangeSession::new_qr(alice_identity, ContactCard::new("Alice"), mock_proximity());
    let mut bob = ExchangeSession::new_qr(bob_identity, ContactCard::new("Bob"), mock_proximity());

    // Both display their QR codes
    alice.apply(ExchangeEvent::StartQR).unwrap();
    bob.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice.qr().unwrap().clone();
    let bob_qr = bob.qr().unwrap().clone();

    // Each scans the other's QR
    alice.apply(ExchangeEvent::ProcessQR(bob_qr)).unwrap();
    bob.apply(ExchangeEvent::ProcessQR(alice_qr)).unwrap();

    // Signal that the peer scanned our QR
    alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Key agreement
    alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    (alice, bob)
}

// =============================================================================
// Self-Exchange Prevention Tests
// =============================================================================

/// Scenario: Scanning own QR code should fail with SelfExchange error.
///
/// In the new flow both parties create sessions with `new_qr()`. If a
/// user somehow scans their own QR the session must reject it because
/// the QR's signing public key matches the session's identity.
// @scenario: contact_exchange.feature:Cannot exchange with yourself
#[test]
fn test_self_exchange_rejected() {
    let alice = Identity::create("Alice");

    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    // Display our QR
    session.apply(ExchangeEvent::StartQR).unwrap();
    let our_qr = session.qr().unwrap().clone();

    // Attempt to scan our own QR — must fail with SelfExchange
    let result = session.apply(ExchangeEvent::ProcessQR(our_qr));
    assert!(
        matches!(result, Err(ExchangeError::SelfExchange)),
        "Scanning own QR should return SelfExchange error, got: {result:?}"
    );
}

/// Scenario: Different identity scanning QR succeeds normally.
// @scenario: contact_exchange.feature:Successful QR code exchange with proximity
#[test]
fn test_different_identity_exchange_succeeds() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    // Alice generates QR
    let mut alice_session =
        ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    // Bob displays his QR first, then scans Alice's
    let mut bob_session = ExchangeSession::new_qr(bob, ContactCard::new("Bob"), mock_proximity());
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    let result = bob_session.apply(ExchangeEvent::ProcessQR(alice_qr));
    assert!(result.is_ok(), "Different identity should succeed");
    assert!(
        matches!(bob_session.state(), ExchangeState::PeerScanned { .. }),
        "Bob should be in PeerScanned state after scanning Alice's QR"
    );
}

// =============================================================================
// QR Code Expiration Tests
// =============================================================================

/// Scenario: QR code expiration (5 minutes)
// @scenario: contact_exchange.feature:Mutual QR rejects expired peer QR code
#[test]
fn test_qr_expiration() {
    let alice = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    // Fresh QR should not be expired
    let fresh_qr = ExchangeQR::generate(&alice, &ephemeral);
    assert!(!fresh_qr.is_expired());
}

/// Scenario: Expired QR is rejected during ProcessQR
// @scenario: contact_exchange.feature:Mutual QR rejects expired peer QR code
#[test]
fn test_expired_qr_rejected_on_process() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");
    let ephemeral = X3DHKeyPair::generate();

    // Create an expired QR (6 minutes ago)
    let expired_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 360;
    let expired_qr = ExchangeQR::generate_with_timestamp(&alice, &ephemeral, expired_ts);
    assert!(expired_qr.is_expired());

    // Bob starts his session and displays QR
    let mut bob_session = ExchangeSession::new_qr(bob, ContactCard::new("Bob"), mock_proximity());
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Attempt to process expired QR should fail
    let result = bob_session.apply(ExchangeEvent::ProcessQR(expired_qr));
    assert!(
        matches!(result, Err(ExchangeError::QRExpired)),
        "Expired QR should be rejected, got: {result:?}"
    );
}

// =============================================================================
// Duplicate Contact Tests
// =============================================================================

/// Scenario: Exchange with existing contact detected
// @scenario: contact_exchange.feature:Exchange with existing contact shows update option
// @scenario: contact_exchange.feature:Update existing contact via exchange
// @scenario: contact_exchange.feature:Keep existing contact without update
#[test]
fn test_duplicate_contact_detection() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    // Alice generates QR
    let mut alice_session =
        ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    // Bob already has Alice as a contact
    let existing_alice = vauchi_core::Contact::from_exchange(
        *alice_qr.public_key(),
        ContactCard::new("Alice"),
        vauchi_core::SymmetricKey::generate(),
    );

    // Bob starts his session, displays QR, then scans Alice's QR
    let mut bob_session = ExchangeSession::new_qr(bob, ContactCard::new("Bob"), mock_proximity());
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Check for duplicate
    let contacts = [existing_alice];
    let duplicate = bob_session.check_duplicate(&contacts);
    assert!(duplicate.is_some());
    assert_eq!(duplicate.unwrap().display_name(), "Alice");
}

/// Scenario: No duplicate detected for new contact
// @scenario: contact_exchange.feature:Exchange with existing contact shows update option
#[test]
fn test_no_duplicate_for_new_contact() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");
    let charlie = Identity::create("Charlie");

    // Bob already has Charlie, not Alice
    let existing_charlie = vauchi_core::Contact::from_exchange(
        *charlie.signing_public_key(),
        ContactCard::new("Charlie"),
        vauchi_core::SymmetricKey::generate(),
    );

    // Alice generates QR
    let mut alice_session =
        ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    // Bob displays QR, scans Alice's
    let mut bob_session = ExchangeSession::new_qr(bob, ContactCard::new("Bob"), mock_proximity());
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    let contacts = [existing_charlie];
    let duplicate = bob_session.check_duplicate(&contacts);
    assert!(
        duplicate.is_none(),
        "Alice should not be detected as duplicate"
    );
}

// =============================================================================
// Session Timeout Tests
// =============================================================================

/// Scenario: Session timeout detection
// @scenario: contact_exchange.feature:Exchange session timeout
#[test]
fn test_session_timeout_detection() {
    let alice = Identity::create("Alice");
    let session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    // Fresh session should not be timed out
    assert!(!session.is_timed_out());
}

/// Scenario: Interrupted session can be resumed within window
// @scenario: contact_exchange.feature:Exchange session timeout
// @scenario: contact_exchange.feature:Resume interrupted exchange
#[test]
fn test_interrupted_session_resumable() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    // Fresh session cannot be resumed (not interrupted)
    assert!(!session.can_resume());

    // Mark as interrupted
    session.mark_interrupted();

    // Now can be resumed (within timeout)
    assert!(session.can_resume());
}

// =============================================================================
// Invalid State Transition Tests
// =============================================================================

/// Scenario: Cannot ProcessQR from Idle state (must call StartQR first)
#[test]
fn test_cannot_process_qr_from_idle() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");
    let ephemeral = X3DHKeyPair::generate();

    // Generate a QR from Bob for Alice to scan
    let bob_qr = ExchangeQR::generate(&bob, &ephemeral);

    // Alice is in Idle state — never called StartQR
    let mut alice_session =
        ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    let result = alice_session.apply(ExchangeEvent::ProcessQR(bob_qr));
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "ProcessQR from Idle should fail, got: {result:?}"
    );
}

/// Scenario: Cannot call StartQR twice (already in DisplayingQr)
#[test]
fn test_cannot_start_qr_twice() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    session.apply(ExchangeEvent::StartQR).unwrap();

    // Second StartQR should fail — already in DisplayingQr
    let result = session.apply(ExchangeEvent::StartQR);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "Double StartQR should fail, got: {result:?}"
    );
}

/// Scenario: Cannot perform key agreement from Idle
#[test]
fn test_cannot_key_agreement_from_idle() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    let result = session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "PerformKeyAgreement from Idle should fail, got: {result:?}"
    );
}

/// Scenario: Cannot complete exchange from Idle
#[test]
fn test_cannot_complete_exchange_from_idle() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    let result = session.apply(ExchangeEvent::CompleteExchange(ContactCard::new("Bob")));
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "CompleteExchange from Idle should fail, got: {result:?}"
    );
}

/// Scenario: Cannot TheyScannedOurQR from DisplayingQr (must be in PeerScanned)
#[test]
fn test_cannot_they_scanned_from_displaying_qr() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    session.apply(ExchangeEvent::StartQR).unwrap();

    let result = session.apply(ExchangeEvent::TheyScannedOurQR);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "TheyScannedOurQR from DisplayingQr (before ProcessQR) should fail, got: {result:?}"
    );
}

/// Scenario: Cannot ProcessQR from PeerScanned (already scanned one)
#[test]
fn test_cannot_process_qr_from_peer_scanned() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    let mut alice_session =
        ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    let mut bob_session = ExchangeSession::new_qr(bob, ContactCard::new("Bob"), mock_proximity());
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Bob scans Alice's QR -> PeerScanned
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr.clone()))
        .unwrap();
    assert!(matches!(
        bob_session.state(),
        ExchangeState::PeerScanned { .. }
    ));

    // Attempt to process another QR from PeerScanned should fail
    let result = bob_session.apply(ExchangeEvent::ProcessQR(alice_qr));
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "ProcessQR from PeerScanned should fail, got: {result:?}"
    );
}

// =============================================================================
// Card Access Tests
// =============================================================================

/// Scenario: Our card is accessible during exchange
#[test]
fn test_our_card_accessible() {
    let alice = Identity::create("Alice");
    let card = ContactCard::new("Alice Card");
    let session = ExchangeSession::new_qr(alice, card, mock_proximity());

    assert_eq!(session.our_card().display_name(), "Alice Card");
}

// =============================================================================
// Signature Verification Tests
// =============================================================================

/// Scenario: Generated QR has valid signature
#[test]
fn test_qr_signature_valid() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    session.apply(ExchangeEvent::StartQR).unwrap();

    let qr = session.qr().unwrap();
    assert!(
        qr.verify_signature(),
        "Generated QR should have valid signature"
    );
}

/// Scenario: QR with invalid signature rejected during ProcessQR
///
/// The session checks `verify_signature()` on the incoming QR. We test
/// this indirectly: a validly-generated QR from a different identity
/// passes, while a maliciously-constructed QR would fail. Here we
/// verify the positive path since we cannot easily tamper with signature
/// bytes through the public API (the QR struct fields are private).
#[test]
fn test_valid_signature_accepted() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    let mut alice_session =
        ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    // Verify the QR has valid signature before scanning
    assert!(alice_qr.verify_signature());

    // Bob should accept it
    let mut bob_session = ExchangeSession::new_qr(bob, ContactCard::new("Bob"), mock_proximity());
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    let result = bob_session.apply(ExchangeEvent::ProcessQR(alice_qr));
    assert!(
        result.is_ok(),
        "Valid QR with good signature should be accepted"
    );
}

// =============================================================================
// QR Reuse Prevention Tests
// =============================================================================

/// Scenario: QR reuse is detected by check_qr_reuse
// @scenario: contact_exchange.feature:Same QR scanned twice by same person
#[test]
fn test_qr_reuse_detected() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    let qr_hash = [42u8; 32];

    // First use succeeds
    assert!(session.check_qr_reuse(&qr_hash).is_ok());

    // Second use with same hash should fail
    let result = session.check_qr_reuse(&qr_hash);
    assert!(
        matches!(result, Err(ExchangeError::QRAlreadyUsed)),
        "Reused QR hash should be rejected, got: {result:?}"
    );
}

/// Scenario: Different QR hashes are independent
#[test]
fn test_different_qr_hashes_independent() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    let hash_a = [1u8; 32];
    let hash_b = [2u8; 32];

    assert!(session.check_qr_reuse(&hash_a).is_ok());
    assert!(session.check_qr_reuse(&hash_b).is_ok());
}

// =============================================================================
// Full Flow Completion Tests
// =============================================================================

/// Scenario: Complete exchange flow produces Complete state
// @scenario: contact_exchange.feature:Successful QR code exchange with proximity
// @scenario: contact_exchange.feature:Exchange creates mutual keys
#[test]
fn test_complete_exchange_flow() {
    let (mut alice_session, mut bob_session) = advance_to_card_exchange();

    // Both should be in AwaitingCardExchange
    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingCardExchange { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingCardExchange { .. }
    ));

    // Complete exchange
    alice_session
        .apply(ExchangeEvent::CompleteExchange(ContactCard::new("Bob")))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(ContactCard::new("Alice")))
        .unwrap();

    assert!(
        matches!(alice_session.state(), ExchangeState::Complete { .. }),
        "Alice should be in Complete state"
    );
    assert!(
        matches!(bob_session.state(), ExchangeState::Complete { .. }),
        "Bob should be in Complete state"
    );
}

// =============================================================================
// Explicit Fail Tests
// =============================================================================

/// Scenario: Session can be explicitly failed from any state
#[test]
fn test_explicit_fail_from_idle() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    session
        .apply(ExchangeEvent::Fail(ExchangeError::SessionTimeout))
        .unwrap();

    assert!(matches!(session.state(), ExchangeState::Failed { .. }));
}

/// Scenario: Session can be explicitly failed from DisplayingQr
#[test]
fn test_explicit_fail_from_displaying_qr() {
    let alice = Identity::create("Alice");
    let mut session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());
    session.apply(ExchangeEvent::StartQR).unwrap();

    session
        .apply(ExchangeEvent::Fail(ExchangeError::Interrupted))
        .unwrap();

    assert!(matches!(session.state(), ExchangeState::Failed { .. }));
}

// =============================================================================
// Transport Enforcement Tests
// =============================================================================

/// Scenario: QR events are rejected on NFC transport sessions
#[test]
fn test_qr_events_rejected_on_nfc_transport() {
    let alice = Identity::create("Alice");
    let mut nfc_session =
        ExchangeSession::new_nfc(alice, ContactCard::new("Alice"), mock_proximity());

    // StartQR should fail on NFC transport
    let result = nfc_session.apply(ExchangeEvent::StartQR);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "StartQR should be rejected on NFC transport, got: {result:?}"
    );
}

/// Scenario: QR events are rejected on BLE transport sessions
#[test]
fn test_qr_events_rejected_on_ble_transport() {
    let alice = Identity::create("Alice");
    let mut ble_session =
        ExchangeSession::new_ble(alice, ContactCard::new("Alice"), mock_proximity());

    // StartQR should fail on BLE transport
    let result = ble_session.apply(ExchangeEvent::StartQR);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "StartQR should be rejected on BLE transport, got: {result:?}"
    );
}

// =============================================================================
// Exchange Public Key Accessibility
// =============================================================================

/// Scenario: Our exchange public key is accessible
#[test]
fn test_our_exchange_public_key_accessible() {
    let alice = Identity::create("Alice");
    let session = ExchangeSession::new_qr(alice, ContactCard::new("Alice"), mock_proximity());

    // Should return a non-zero 32-byte key
    let key = session.our_exchange_public_key();
    assert_ne!(
        key, &[0u8; 32],
        "Exchange public key should not be all zeros"
    );
}
