// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ExchangeDebugLog integration with ExchangeSession.
//!
//! Verifies that when debug logging is enabled on an ExchangeSession,
//! exchange flow events are captured at each state transition.

#![cfg(feature = "testing")]

use vauchi_core::diagnostic::exchange_debug::ExchangeDebugEvent;
use vauchi_core::exchange::*;
use vauchi_core::*;

// ===== Debug log activation =====

#[test]
fn debug_log_disabled_by_default() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = ExchangeSession::new_qr(identity, card, MockProximityVerifier::success());

    assert!(session.exchange_debug_log().is_none());
}

#[test]
fn enable_debug_log_creates_log_with_session_started() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_qr(identity, card, MockProximityVerifier::success());

    session.enable_debug_log();

    let log = session.exchange_debug_log().expect("log should exist");
    assert_eq!(log.events().len(), 1);
    assert!(matches!(
        &log.events()[0].event,
        ExchangeDebugEvent::SessionStarted { transport } if transport == "qr"
    ));
}

#[test]
fn enable_debug_log_nfc_records_transport() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_nfc(identity, card, MockProximityVerifier::success());

    session.enable_debug_log();

    let log = session.exchange_debug_log().unwrap();
    assert!(matches!(
        &log.events()[0].event,
        ExchangeDebugEvent::SessionStarted { transport } if transport == "nfc"
    ));
}

#[test]
fn enable_debug_log_ble_records_transport() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_ble(identity, card, MockProximityVerifier::success());

    session.enable_debug_log();

    let log = session.exchange_debug_log().unwrap();
    assert!(matches!(
        &log.events()[0].event,
        ExchangeDebugEvent::SessionStarted { transport } if transport == "ble"
    ));
}

// ===== QR flow events =====

#[test]
fn start_qr_logs_qr_generated() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_qr(identity, card, MockProximityVerifier::success());
    session.enable_debug_log();

    session.apply(ExchangeEvent::StartQR).unwrap();

    let log = session.exchange_debug_log().unwrap();
    // SessionStarted + QrGenerated
    assert_eq!(log.events().len(), 2);
    assert!(matches!(
        &log.events()[1].event,
        ExchangeDebugEvent::QrGenerated
    ));
}

#[test]
fn process_qr_logs_qr_scanned() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");
    let mut bob_session =
        ExchangeSession::new_qr(bob_identity, bob_card, MockProximityVerifier::success());
    bob_session.enable_debug_log();

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    let log = bob_session.exchange_debug_log().unwrap();
    // SessionStarted + QrGenerated + QrScanned
    assert_eq!(log.events().len(), 3);
    assert!(matches!(
        &log.events()[2].event,
        ExchangeDebugEvent::QrScanned
    ));
}

// ===== Key agreement + proximity =====

#[test]
fn key_agreement_logs_completion_and_proximity() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_ephemeral = X3DHKeyPair::generate();
    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let mut bob_session =
        ExchangeSession::new_qr(bob_identity, bob_card, MockProximityVerifier::success());
    bob_session.enable_debug_log();

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    let log = bob_session.exchange_debug_log().unwrap();

    // SessionStarted, QrGenerated, QrScanned,
    // KeyAgreementCompleted, ProximityCheckStarted, ProximityCheckCompleted
    assert_eq!(log.events().len(), 6);

    // Verify key agreement event exists
    assert!(log
        .events()
        .iter()
        .any(|e| matches!(&e.event, ExchangeDebugEvent::KeyAgreementCompleted)));

    // Verify proximity events exist
    assert!(log
        .events()
        .iter()
        .any(|e| matches!(&e.event, ExchangeDebugEvent::ProximityCheckStarted { .. })));
    assert!(log
        .events()
        .iter()
        .any(|e| matches!(&e.event, ExchangeDebugEvent::ProximityCheckCompleted { .. })));
}

// ===== Exchange completion =====

#[test]
fn complete_exchange_logs_completed() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
    );
    bob_session.enable_debug_log();

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let log = bob_session.exchange_debug_log().unwrap();
    assert!(log
        .events()
        .iter()
        .any(|e| matches!(&e.event, ExchangeDebugEvent::ExchangeCompleted)));
}

// ===== Failure logging =====

#[test]
fn fail_event_logs_exchange_failed() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_qr(identity, card, MockProximityVerifier::success());
    session.enable_debug_log();

    session
        .apply(ExchangeEvent::Fail(ExchangeError::SessionTimeout))
        .unwrap();

    let log = session.exchange_debug_log().unwrap();
    assert!(log.events().iter().any(|e| matches!(
        &e.event,
        ExchangeDebugEvent::ExchangeFailed { error } if error == "Exchange session timed out"
    )));
}

// ===== Idempotency =====

#[test]
fn enable_debug_log_twice_is_idempotent() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_qr(identity, card, MockProximityVerifier::success());

    session.enable_debug_log();
    session.enable_debug_log(); // second call must be a no-op

    let log = session.exchange_debug_log().unwrap();
    assert_eq!(log.events().len(), 1);
    assert!(matches!(
        &log.events()[0].event,
        ExchangeDebugEvent::SessionStarted { .. }
    ));
}

// ===== No events when disabled =====

#[test]
fn no_debug_events_when_log_disabled() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_qr(identity, card, MockProximityVerifier::success());
    // Do NOT call enable_debug_log

    session.apply(ExchangeEvent::StartQR).unwrap();

    assert!(session.exchange_debug_log().is_none());
}

// ===== JSONL export =====

#[test]
fn debug_log_exports_to_jsonl() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_qr(identity, card, MockProximityVerifier::success());
    session.enable_debug_log();

    session.apply(ExchangeEvent::StartQR).unwrap();

    let log = session.exchange_debug_log().unwrap();
    let jsonl = log.to_jsonl();
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 2); // SessionStarted + QrGenerated

    for line in &lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("Invalid JSON: {}: {}", line, e));
        parsed.get("elapsed_ms").expect("expected Some");
    }
}
