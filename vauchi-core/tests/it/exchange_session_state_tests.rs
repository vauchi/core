// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange session state a frontend or contact record relies on: the
//! 60-second session timeout, the peer's name and the shared key after key
//! agreement, and the proximity confidence reported and logged
//! (vauchi/private#522).

#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::Duration;
use vauchi_core::diagnostic::exchange_debug::ExchangeDebugEvent;
use vauchi_core::exchange::*;
use vauchi_core::monotonic::FakeMonotonicClock;
use vauchi_core::*;

fn bob_session() -> ExchangeSession {
    ExchangeSession::new_qr(
        Identity::create("Bob", 0),
        ContactCard::new("Bob"),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    )
}

/// Bob after scanning Alice's QR and agreeing a key with her.
fn bob_after_key_agreement() -> ExchangeSession {
    let alice_qr = ExchangeQR::generate(
        &Identity::create("Alice", 0),
        &X3DHKeyPair::generate(),
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bob = bob_session();
    bob.enable_debug_log();
    bob.apply(ExchangeEvent::StartQR).unwrap();
    bob.apply(ExchangeEvent::ProcessQR(alice_qr)).unwrap();
    bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    bob
}

// @internal
#[test]
fn after_key_agreement_the_session_knows_the_peer_and_holds_a_key() {
    let bob = bob_after_key_agreement();

    assert_eq!(bob.their_display_name(), Some("Alice"));
    assert!(bob.shared_key().is_some());
}

// @internal
#[test]
fn the_proximity_check_s_confidence_is_reported_and_logged() {
    let mut bob = bob_after_key_agreement();

    let logged: Vec<String> = bob
        .exchange_debug_log()
        .unwrap()
        .events()
        .iter()
        .filter_map(|e| match &e.event {
            ExchangeDebugEvent::ProximityCheckCompleted { confidence } => Some(confidence.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(logged, ["high"]);
    assert_eq!(bob.proximity_confidence(), ProximityConfidence::High);

    bob.apply(ExchangeEvent::ProximityCheckCompleted {
        confidence: ProximityConfidence::Medium,
    })
    .unwrap();
    assert_eq!(bob.proximity_confidence(), ProximityConfidence::Medium);
}

// @internal
#[test]
fn a_session_accepts_events_for_sixty_seconds_then_fails() {
    let clock = Arc::new(FakeMonotonicClock::new());
    let mut bob = bob_session().with_monotonic(clock.clone());

    clock.advance(Duration::from_secs(60));
    assert!(!bob.is_timed_out());
    bob.apply(ExchangeEvent::StartQR).unwrap();

    clock.advance(Duration::from_millis(1));
    assert!(bob.is_timed_out());
    assert!(matches!(
        bob.apply(ExchangeEvent::TheyScannedOurQR),
        Err(ExchangeError::SessionTimeout)
    ));
    assert!(matches!(bob.state(), ExchangeState::Failed { .. }));
}

// @internal
#[test]
fn a_timed_out_session_can_still_be_failed_explicitly() {
    let clock = Arc::new(FakeMonotonicClock::new());
    let mut bob = bob_session().with_monotonic(clock.clone());
    clock.advance(Duration::from_secs(61));

    assert!(
        bob.apply(ExchangeEvent::Fail(ExchangeError::SessionTimeout))
            .is_ok()
    );
}

fn ble_session() -> ExchangeSession {
    ExchangeSession::new_ble(
        Identity::create("Bob", 0),
        ContactCard::new("Bob"),
        ManualConfirmationVerifier::new(),
        vauchi_core::clock::SystemClock::shared(),
    )
}

// @internal
#[test]
fn a_ble_exchange_starts_only_on_a_ble_session_before_it_connects() {
    let mut qr = bob_session();
    assert!(matches!(
        qr.apply(ExchangeEvent::StartBleExchange),
        Err(ExchangeError::InvalidState(_))
    ));

    let mut ble = ble_session();
    ble.apply(ExchangeEvent::StartBleExchange).unwrap();
    assert!(matches!(ble.state(), ExchangeState::AwaitingBleConnection));
    ble.apply(ExchangeEvent::StartBleExchange).unwrap();

    ble.apply(ExchangeEvent::Fail(ExchangeError::SessionTimeout))
        .unwrap();
    assert!(matches!(
        ble.apply(ExchangeEvent::StartBleExchange),
        Err(ExchangeError::InvalidState(_))
    ));
}
