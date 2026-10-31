// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a completed exchange records: the contact's exchange time is the
//! session clock's, not zero; in a mutual QR exchange one side emits the
//! audio challenge first and the other listens first; and a proximity
//! confidence the shell confirmed is not lowered by the automatic check
//! that follows key agreement (vauchi/private#948).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeSession, ExchangeState, MockProximityVerifier, ProximityConfidence,
};
use vauchi_core::identity::Identity;
use vauchi_core::platform::Command;

const T0: u64 = 1_700_000_000;

fn clock_at(secs: u64) -> Arc<dyn Clock> {
    FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)).shared()
}

fn qr_session(
    name: &str,
    verifier: MockProximityVerifier,
    clock: Arc<dyn Clock>,
) -> ExchangeSession {
    ExchangeSession::new_qr(
        Identity::create(name, 0),
        ContactCard::new(name),
        verifier,
        clock,
    )
}

/// Both sides through the mutual QR flow up to and including key agreement.
fn both_scanned_and_agreed(
    mut alice: ExchangeSession,
    mut bob: ExchangeSession,
) -> (ExchangeSession, ExchangeSession) {
    alice.apply(ExchangeEvent::StartQR).unwrap();
    bob.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice.qr().unwrap().clone();
    let bob_qr = bob.qr().unwrap().clone();
    alice.apply(ExchangeEvent::ProcessQR(bob_qr)).unwrap();
    bob.apply(ExchangeEvent::ProcessQR(alice_qr)).unwrap();
    alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    alice.drain_commands();
    bob.drain_commands();
    alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    (alice, bob)
}

fn completed_contact(session: &ExchangeSession) -> vauchi_core::contact::Contact {
    match session.state() {
        ExchangeState::Complete { contact } => (**contact).clone(),
        other => panic!("expected Complete, got {other:?}"),
    }
}

// @scenario: contact_exchange :: A QR contact carries its real exchange timestamp
#[test]
fn a_qr_session_stores_the_exchange_timestamp() {
    let alice = qr_session("Alice", MockProximityVerifier::success(), clock_at(T0));
    let bob = qr_session("Bob", MockProximityVerifier::success(), clock_at(T0 + 5));
    let (mut alice, mut bob) = both_scanned_and_agreed(alice, bob);

    alice
        .apply(ExchangeEvent::CompleteExchange(ContactCard::new("Bob")))
        .unwrap();
    bob.apply(ExchangeEvent::CompleteExchange(ContactCard::new("Alice")))
        .unwrap();

    assert_eq!(
        completed_contact(&alice).exchange_timestamp(),
        Some(T0),
        "the contact is stamped with the session clock, not 0"
    );
    assert_eq!(completed_contact(&bob).exchange_timestamp(), Some(T0 + 5));
}

/// Which audio command a side emits first, if any.
fn first_audio_step(commands: &[Command]) -> Option<&'static str> {
    commands.iter().find_map(|command| match command {
        Command::AudioEmitChallenge { .. } => Some("emit"),
        Command::AudioListenForResponse { .. } => Some("listen"),
        _ => None,
    })
}

// @scenario: contact_exchange :: In a mutual QR exchange one side emits the audio challenge first
#[test]
fn in_a_mutual_qr_exchange_one_side_emits_first_and_the_other_listens_first() {
    let alice = qr_session("Alice", MockProximityVerifier::success(), clock_at(T0));
    let bob = qr_session("Bob", MockProximityVerifier::success(), clock_at(T0));
    let (mut alice, mut bob) = both_scanned_and_agreed(alice, bob);

    let alice_first = first_audio_step(&alice.drain_commands());
    let bob_first = first_audio_step(&bob.drain_commands());

    let mut steps = [alice_first, bob_first];
    steps.sort();
    assert_eq!(
        steps,
        [Some("emit"), Some("listen")],
        "exactly one side emits first, the other listens first; got alice {alice_first:?}, bob {bob_first:?}"
    );
}

// @scenario: contact_exchange :: A confirmed proximity is not lowered by the automatic check
#[test]
fn a_confirmed_medium_proximity_is_not_lowered_by_the_key_agreement_check() {
    let alice = qr_session("Alice", MockProximityVerifier::failure(), clock_at(T0));
    let bob = qr_session("Bob", MockProximityVerifier::success(), clock_at(T0));
    let mut alice_with_bob = {
        let (mut alice, mut bob) = (alice, bob);
        alice.apply(ExchangeEvent::StartQR).unwrap();
        bob.apply(ExchangeEvent::StartQR).unwrap();
        let bob_qr = bob.qr().unwrap().clone();
        alice.apply(ExchangeEvent::ProcessQR(bob_qr)).unwrap();
        alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
        alice
    };
    alice_with_bob
        .apply(ExchangeEvent::ProximityCheckCompleted {
            confidence: ProximityConfidence::Medium,
        })
        .unwrap();

    alice_with_bob
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    assert_eq!(
        alice_with_bob.proximity_confidence(),
        ProximityConfidence::Medium,
        "the shell's confirmation stands; a failing automatic check does not lower it"
    );
    alice_with_bob
        .apply(ExchangeEvent::CompleteExchange(ContactCard::new("Bob")))
        .unwrap();
    assert_eq!(
        *completed_contact(&alice_with_bob).proximity_confidence(),
        ProximityConfidence::Medium
    );
}
