// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::*;
use vauchi_core::*;

// @scenario: contact_exchange.feature:QR code exchange blocked without proximity
#[test]
fn test_lazy_frontend_skips_they_scanned() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session = ExchangeSession::new_qr(alice_identity, alice_card, alice_proximity);

    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");
    let bob_proximity = MockProximityVerifier::failure();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, bob_proximity);

    // Bob starts his QR display too
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // 1. Bob scans Alice's QR -> moves to PeerScanned
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // 2. Bob's frontend is "lazy" and doesn't wait for TheyScannedOurQR
    // but tries to call perform_key_agreement() directly.

    let res = bob_session.apply(ExchangeEvent::PerformKeyAgreement);

    assert!(
        res.is_err(),
        "Should NOT be able to perform key agreement without TheyScannedOurQR"
    );
}

// @scenario: contact_exchange.feature:QR code exchange blocked without proximity
#[test]
fn test_formalized_state_machine() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let mut alice_session =
        ExchangeSession::new_qr(alice_identity, alice_card, MockProximityVerifier::success());

    // Test transition using apply
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    assert!(matches!(
        alice_session.state(),
        ExchangeState::DisplayingQr { .. }
    ));

    // Test invalid transition
    let res = alice_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(res.is_err(), "expected error");
    assert!(matches!(res.unwrap_err(), ExchangeError::InvalidState(_)));
}
