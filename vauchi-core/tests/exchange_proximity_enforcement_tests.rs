// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Proximity Enforcement Integration Tests
//!
//! Verifies that the exchange session state machine correctly enforces
//! proximity verification for QR one-way transport and correctly skips
//! it for mutual QR and NFC transports (where proximity is guaranteed
//! by the transport mechanism itself).
//!
//! These tests require the `testing` feature because they use
//! `ManualConfirmationVerifier::pre_confirmed()` which is gated.

use vauchi_core::exchange::{
    ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState,
    ManualConfirmationVerifier, MockProximityVerifier,
};
use vauchi_core::*;

/// QR one-way: attempting key agreement without proximity verification must fail.
#[test]
fn test_qr_oneway_key_agreement_without_proximity_fails() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    // Alice generates QR
    let alice_qr = ExchangeQR::generate(&alice);

    // Bob processes QR — should land in AwaitingProximity
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::failure();
    let mut bob_session = ExchangeSession::new_responder(bob, bob_card, proximity);

    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    assert!(
        matches!(bob_session.state(), ExchangeState::AwaitingProximity { .. }),
        "Should be in AwaitingProximity after ProcessQR"
    );

    // Attempting key agreement directly should fail — proximity not verified
    let result = bob_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        result.is_err(),
        "Key agreement without proximity should fail"
    );
}

/// QR one-way: key agreement after proximity verification must succeed.
#[test]
fn test_qr_oneway_key_agreement_after_proximity_succeeds() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_responder(bob, bob_card, proximity);

    // Process QR -> AwaitingProximity
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingProximity { .. }
    ));

    // Verify proximity -> AwaitingKeyAgreement
    bob_session.apply(ExchangeEvent::VerifyProximity).unwrap();

    assert!(
        matches!(
            bob_session.state(),
            ExchangeState::AwaitingKeyAgreement { .. }
        ),
        "Should be in AwaitingKeyAgreement after proximity verification"
    );

    // Key agreement should now succeed
    let result = bob_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        result.is_ok(),
        "Key agreement after proximity should succeed"
    );
}

/// Mutual QR: does not require proximity verification step (both parties
/// display and scan, which inherently proves physical presence).
#[test]
fn test_mutual_qr_skips_proximity() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    // Generate QR codes before consuming identities into sessions
    let alice_qr = ExchangeQR::generate(&alice);
    let bob_qr = ExchangeQR::generate(&bob);

    // Alice starts mutual QR
    let alice_card = ContactCard::new("Alice");
    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session = ExchangeSession::new_mutual_qr(alice, alice_card, alice_proximity);

    alice_session.apply(ExchangeEvent::StartMutualQR).unwrap();

    // Bob starts mutual QR
    let bob_card = ContactCard::new("Bob");
    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_mutual_qr(bob, bob_card, bob_proximity);

    bob_session.apply(ExchangeEvent::StartMutualQR).unwrap();

    // Bob scans Alice's QR
    bob_session
        .apply(ExchangeEvent::ScannedTheirQR(alice_qr))
        .unwrap();

    // Alice scans Bob's QR
    alice_session
        .apply(ExchangeEvent::ScannedTheirQR(bob_qr))
        .unwrap();

    // Signal that they scanned our QR (in real flow, this comes from relay)
    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();

    // Key agreement should work without explicit VerifyProximity
    let alice_result = alice_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        alice_result.is_ok(),
        "Mutual QR should not require explicit proximity step: {:?}",
        alice_result
    );
}

/// NFC: does not require proximity verification step (physical tap IS proximity).
#[test]
fn test_nfc_skips_proximity() {
    let alice = Identity::create("Alice");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();
    let mut session = ExchangeSession::new_nfc(alice, alice_card, proximity);

    // NFC tap with mock payload
    let their_payload = vec![0u8; 64]; // Minimal valid-ish payload
    let result = session.apply(ExchangeEvent::NfcTapComplete { their_payload });

    // NFC tap should transition directly — no proximity step needed
    // The result might fail due to invalid payload format, but it should NOT
    // fail due to "proximity not verified" — it should attempt to parse the payload
    if let Err(ref e) = result {
        // InvalidNfcFormat is acceptable (mock payload), but InvalidState would mean
        // proximity is incorrectly required
        assert!(
            !matches!(e, ExchangeError::InvalidState { .. }),
            "NFC should not require proximity verification step, got: {:?}",
            e
        );
    }
}

/// ManualConfirmationVerifier::pre_confirmed is available in testing mode.
/// This test just verifies the cfg gate works.
#[test]
fn test_pre_confirmed_available_in_testing() {
    let verifier = ManualConfirmationVerifier::pre_confirmed();
    assert!(verifier.is_confirmed());
}
