// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Proximity Enforcement Integration Tests
//!
//! Verifies that the exchange session state machine correctly handles
//! QR exchange (both scan = implicit proximity) and NFC transports
//! (where proximity is guaranteed by the transport mechanism itself).
//!
//! These tests require the `testing` feature because they use
//! `ManualConfirmationVerifier::pre_confirmed()` which is gated.

use vauchi_core::exchange::{
    ExchangeError, ExchangeEvent, ExchangeSession, ExchangeState, ManualConfirmationVerifier,
    MockProximityVerifier,
};
use vauchi_core::*;

/// QR exchange: both sides display and scan — implicit proximity.
/// Key agreement after scanning and confirming should succeed.
// @scenario: contact_exchange :: Successful QR code exchange with proximity
// @internal
#[test]
fn test_qr_exchange_key_agreement_succeeds() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let alice_proximity = MockProximityVerifier::success();
    let bob_proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_qr(
        alice,
        alice_card,
        alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob,
        bob_card,
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Both start QR
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Get QR codes
    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    // Both scan each other's QR
    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    assert!(
        matches!(alice_session.state(), ExchangeState::PeerScanned { .. }),
        "Should be in PeerScanned after scanning"
    );

    // Confirm other party scanned
    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();

    assert!(
        matches!(
            alice_session.state(),
            ExchangeState::AwaitingKeyAgreement { .. }
        ),
        "Should be in AwaitingKeyAgreement after TheyScannedOurQR"
    );

    // Key agreement should succeed
    let result = alice_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        result.is_ok(),
        "QR exchange key agreement should succeed: {:?}",
        result
    );
}

/// QR exchange: attempting key agreement from wrong state should fail.
// @scenario: contact_exchange :: QR code exchange blocked without proximity
// @internal
#[test]
fn test_qr_key_agreement_from_wrong_state_fails() {
    let alice = Identity::create("Alice", 0);
    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_qr(
        alice,
        alice_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Try key agreement from Idle — should fail
    let result = session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(result.is_err(), "Key agreement from Idle should fail");

    // Start QR then try key agreement from DisplayingQr — should fail
    session.apply(ExchangeEvent::StartQR).unwrap();
    let result = session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        result.is_err(),
        "Key agreement from DisplayingQr should fail"
    );
}

/// NFC: does not require proximity verification step (physical tap IS proximity).
// @scenario: contact_exchange :: NFC active exchange between two phones
// @internal
#[test]
fn test_nfc_skips_proximity() {
    let alice = Identity::create("Alice", 0);

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();
    let mut session = ExchangeSession::new_nfc(
        alice,
        alice_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

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
// @internal
#[test]
fn test_pre_confirmed_available_in_testing() {
    let verifier = ManualConfirmationVerifier::pre_confirmed();
    assert!(verifier.is_confirmed());
}
