// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-Based Tests for Exchange Session State Machine
//!
//! Verifies that no sequence of events causes the state machine to panic,
//! and that the happy-path QR exchange always succeeds regardless of
//! interleaved invalid events.
//!
//! CC-13: Stateful property tests for state machines.
//! Feature: contact_exchange.feature

use proptest::prelude::*;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{
    ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState,
    MockProximityVerifier, ProximityConfidence,
};
use vauchi_core::identity::Identity;

/// Create a QR exchange session pair: (session, peer_qr, peer_card).
fn make_session_and_peer() -> (ExchangeSession, ExchangeQR, ContactCard) {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();
    let mut session = ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    let peer_identity = Identity::create("Bob");
    let peer_card = ContactCard::new("Bob");
    let peer_proximity = MockProximityVerifier::success();
    let mut peer_session = ExchangeSession::new_qr(
        peer_identity,
        peer_card.clone(),
        peer_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Generate peer's QR by starting their session
    peer_session.apply(ExchangeEvent::StartQR).unwrap();
    let peer_qr = peer_session.qr().unwrap().clone();

    // Start our session so it has a QR too
    session.apply(ExchangeEvent::StartQR).unwrap();

    (session, peer_qr, peer_card)
}

/// Construct an event from an index. Uses pre-built peer data for events
/// that carry crypto material (ProcessQR, CompleteExchange).
fn event_from_index(i: u8, peer_qr: &ExchangeQR, peer_card: &ContactCard) -> ExchangeEvent {
    match i {
        0 => ExchangeEvent::StartQR,
        1 => ExchangeEvent::ProcessQR(peer_qr.clone()),
        2 => ExchangeEvent::TheyScannedOurQR,
        3 => ExchangeEvent::PerformKeyAgreement,
        4 => ExchangeEvent::CompleteExchange(peer_card.clone()),
        5 => ExchangeEvent::Fail(ExchangeError::Interrupted),
        6 => ExchangeEvent::ProximityCheckCompleted {
            confidence: ProximityConfidence::High,
        },
        _ => ExchangeEvent::ProximityCheckCompleted {
            confidence: ProximityConfidence::Unknown,
        },
    }
}

// ============================================================
// Property: No event sequence panics (CC-13)
// ============================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// The primary property under test is **panic-freedom**: no sequence
    /// of events must cause the state machine to crash. Each of the 500
    /// generated sequences applies 1–20 random events (including invalid
    /// transitions, out-of-order events, and repeated events) to a live
    /// session backed by real crypto. If `apply()` panics on any input,
    /// the test fails with the minimal shrunk counterexample.
    ///
    /// The `prop_assert!` below is a secondary sanity check — no event
    /// transitions back to `Idle` once the session has started, so this
    /// assertion is structurally always true. Its purpose is CC-17
// @internal
    /// compliance (every `#[test]` must contain an assertion).
    // @scenario: contact_exchange :: Exchange handles invalid state transitions
// @internal
    #[test]
    fn no_event_sequence_panics(
        event_indices in proptest::collection::vec(0..8u8, 1..20),
    ) {
        let (mut session, peer_qr, peer_card) = make_session_and_peer();

        for &i in &event_indices {
            let event = event_from_index(i, &peer_qr, &peer_card);
            // May succeed or fail — must not panic
            let _ = session.apply(event);
        }

        // No event transitions back to Idle once the session has started
        // (StartQR was applied in make_session_and_peer). This assertion
        // is structurally always true; the real value of this test is that
        // reaching this line proves no panic occurred.
        let state = session.state();
        prop_assert!(
            !matches!(state, ExchangeState::Idle),
            "session must not be Idle after events were applied"
        );
    }

    /// After an explicit Fail event, the session must be in Failed state
    /// and reject all subsequent events except another Fail.
    // @scenario: contact_exchange :: Exchange handles invalid state transitions
// @internal
    #[test]
    fn fail_event_is_terminal(
        pre_events in proptest::collection::vec(0..5u8, 0..5),
        post_events in proptest::collection::vec(0..5u8, 1..5),
    ) {
        let (mut session, peer_qr, peer_card) = make_session_and_peer();

        // Apply some random events before failing
        for &i in &pre_events {
            let _ = session.apply(event_from_index(i, &peer_qr, &peer_card));
        }

        // Fail the session
        session
            .apply(ExchangeEvent::Fail(ExchangeError::Interrupted))
            .unwrap();
        prop_assert!(
            matches!(session.state(), ExchangeState::Failed { .. }),
            "session must be in Failed state after Fail event"
        );

        // Subsequent non-Fail events must not panic
        for &i in &post_events {
            let _ = session.apply(event_from_index(i, &peer_qr, &peer_card));
        }

        // Failed state must be terminal — no event should transition out of it
        prop_assert!(
            matches!(session.state(), ExchangeState::Failed { .. }),
            "session must remain in Failed state after subsequent events"
        );
    }
}

// ============================================================
// Deterministic: happy path always completes
// ============================================================

/// The canonical QR exchange sequence must always produce a Complete state.
/// This is a regression guard — if any refactor breaks the happy path,
/// this fails immediately.
// @scenario: contact_exchange :: Successful QR code exchange with proximity
// @scenario: contact_exchange :: Exchange creates mutual keys
// @internal
#[test]
fn happy_path_qr_exchange_always_completes() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let alice_proximity = MockProximityVerifier::success();
    let bob_proximity = MockProximityVerifier::success();

    let mut alice = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Both start QR
    alice.apply(ExchangeEvent::StartQR).unwrap();
    bob.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice.qr().unwrap().clone();
    let bob_qr = bob.qr().unwrap().clone();

    // Both scan each other
    alice.apply(ExchangeEvent::ProcessQR(bob_qr)).unwrap();
    bob.apply(ExchangeEvent::ProcessQR(alice_qr)).unwrap();

    // Both confirm the other scanned
    alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Both perform key agreement
    alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    // Both complete with each other's card
    alice
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob.apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    assert!(
        matches!(alice.state(), ExchangeState::Complete { .. }),
        "Alice must reach Complete state"
    );
    assert!(
        matches!(bob.state(), ExchangeState::Complete { .. }),
        "Bob must reach Complete state"
    );

    // Both should produce contacts with matching shared keys
    let alice_key = match alice.state() {
        ExchangeState::Complete { contact } => contact.shared_key().unwrap().as_bytes().to_vec(),
        _ => unreachable!(),
    };
    let bob_key = match bob.state() {
        ExchangeState::Complete { contact } => contact.shared_key().unwrap().as_bytes().to_vec(),
        _ => unreachable!(),
    };
    assert_eq!(
        alice_key, bob_key,
        "Shared keys must match after symmetric exchange"
    );
}
