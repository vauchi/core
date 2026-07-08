// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Trust Metrics Wiring Integration Tests
//!
//! Verifies that `build_trust_metrics()` is called during exchange completion
//! and the resulting Contact has correct trust metrics reflecting the transport
//! and proximity verification used.
//!
//! Feature: contacts_management.feature @contacts

use vauchi_core::Identity;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, ExchangeTransport,
    MockProximityVerifier, ProximityConfidence, TransportProximity, X3DHKeyPair,
};

// ── Helpers ──────────────────────────────────────────────────────────

/// Runs a full QR exchange ceremony and returns the contact from Bob's perspective.
fn run_full_qr_exchange_with_mock() -> Contact {
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
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session.run_proximity_check();

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => *contact.clone(),
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// ============================================================
// Wiring: QR exchange produces trust metrics
// ============================================================

// @scenario: contact_exchange :: Completed QR exchange contact has trust metrics
#[test]
fn test_completed_qr_exchange_contact_has_trust_metrics() {
    let contact = run_full_qr_exchange_with_mock();

    let metrics = contact
        .trust_metrics()
        .expect("Completed exchange contact must have trust_metrics");

    assert_eq!(
        metrics.transport,
        ExchangeTransport::Qr,
        "Trust metrics transport must match the session transport"
    );
    assert_eq!(
        metrics.transport_proximity,
        TransportProximity::None,
        "QR transport has no inherent proximity guarantee"
    );
    assert_eq!(
        metrics.proximity,
        ProximityConfidence::High,
        "MockProximityVerifier::success() yields High confidence"
    );
    assert!(
        metrics.timestamp > 0,
        "Exchange timestamp must be a positive Unix epoch"
    );
}

// ============================================================
// Wiring: TransportProximity derivation
// ============================================================

// @scenario: contact_exchange :: Transport proximity is derived correctly for each transport type
#[test]
fn test_transport_proximity_derivation() {
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Usb),
        TransportProximity::Physical,
        "USB must be Physical proximity"
    );
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Nfc),
        TransportProximity::ContactRange,
        "NFC must be ContactRange proximity"
    );
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Ble),
        TransportProximity::Proximate,
        "BLE must be Proximate"
    );
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Qr),
        TransportProximity::None,
        "QR must have no inherent proximity"
    );
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Audio),
        TransportProximity::None,
        "Audio must have no inherent proximity"
    );
}

// @scenario: contact_exchange :: Transport proximity strength classification
#[test]
fn test_transport_proximity_strength() {
    assert!(
        TransportProximity::Physical.is_strong(),
        "Physical proximity is strong"
    );
    assert!(
        TransportProximity::ContactRange.is_strong(),
        "ContactRange proximity is strong"
    );
    assert!(
        !TransportProximity::Proximate.is_strong(),
        "Proximate (BLE) is not strong without additional verification"
    );
    assert!(
        !TransportProximity::None.is_strong(),
        "No proximity is not strong"
    );
}

// ============================================================
// Wiring: Mutual exchange — both sides get trust metrics
// ============================================================

// @scenario: contact_exchange :: Both sides of a mutual QR exchange get trust metrics
#[test]
fn test_mutual_qr_exchange_both_sides_have_trust_metrics() {
    let alice_id = Identity::create("Alice", 0);
    let bob_id = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_qr(
        alice_id,
        alice_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob_id,
        bob_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
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

    alice_session.run_proximity_check();
    bob_session.run_proximity_check();

    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let alice_contact = match alice_session.state() {
        ExchangeState::Complete { contact } => *contact.clone(),
        other => panic!("Expected Complete for Alice, got {:?}", other),
    };
    let bob_contact = match bob_session.state() {
        ExchangeState::Complete { contact } => *contact.clone(),
        other => panic!("Expected Complete for Bob, got {:?}", other),
    };

    // Both contacts must have trust metrics
    let alice_metrics = alice_contact
        .trust_metrics()
        .expect("Alice's contact must have trust_metrics");
    let bob_metrics = bob_contact
        .trust_metrics()
        .expect("Bob's contact must have trust_metrics");

    assert_eq!(alice_metrics.transport, ExchangeTransport::Qr);
    assert_eq!(bob_metrics.transport, ExchangeTransport::Qr);

    // Both ran proximity check with MockProximityVerifier::success() -> High
    assert_eq!(alice_metrics.proximity, ProximityConfidence::High);
    assert_eq!(bob_metrics.proximity, ProximityConfidence::High);

    assert!(alice_metrics.timestamp > 0);
    assert!(bob_metrics.timestamp > 0);
}
