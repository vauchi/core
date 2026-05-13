// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ultrasonic audio proximity verification via ADR-031 command/event.
//!
//! After key agreement, ExchangeSession should emit AudioEmitChallenge or
//! AudioListenForResponse commands (depending on initiator role). The frontend
//! handles audio I/O and reports back via AudioResponseReceived.

use vauchi_core::ContactCard;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeSession, ExchangeState, ManualConfirmationVerifier,
};
use vauchi_core::identity::Identity;
use vauchi_core::{Command, Event};

fn qr_session(name: &str) -> ExchangeSession {
    let identity = Identity::create(name);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    )
}

/// Helper: advance two QR sessions to PeerScanned state.
fn create_peer_scanned_sessions() -> (ExchangeSession, ExchangeSession) {
    let mut alice = qr_session("Alice");
    let mut bob = qr_session("Bob");

    alice.apply(ExchangeEvent::StartQR).unwrap();
    bob.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice.qr().unwrap().to_data_string();
    let bob_qr = bob.qr().unwrap().to_data_string();

    // Alice scans Bob's QR
    alice
        .apply_hardware_event(Event::QrScanned { data: bob_qr })
        .unwrap();
    // Bob scans Alice's QR
    bob.apply_hardware_event(Event::QrScanned { data: alice_qr })
        .unwrap();

    (alice, bob)
}

// −− Audio commands emitted after key agreement −−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn qr_key_agreement_emits_audio_commands() {
    let (mut alice, _bob) = create_peer_scanned_sessions();

    // Advance Alice to key agreement
    alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    let _ = alice.drain_commands(); // drain any prior commands

    alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    let cmds = alice.drain_commands();

    // Should emit audio challenge/listen commands for proximity verification
    let has_audio = cmds.iter().any(|c| {
        matches!(
            c,
            Command::AudioEmitChallenge { .. } | Command::AudioListenForResponse { .. }
        )
    });
    assert!(
        has_audio,
        "after key agreement, should emit audio proximity commands, got {:?}",
        cmds
    );
}

// −− AudioResponseReceived sets proximity confidence −−−−−−−−−−−−−−−−−

// @internal
#[test]
fn audio_response_received_advances_state() {
    let (mut alice, _bob) = create_peer_scanned_sessions();

    alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    let _ = alice.drain_commands();

    // Simulate audio response — frontend captured samples at 44.1 kHz.
    alice
        .apply_hardware_event(Event::AudioSamplesRecorded {
            samples: vec![0.1, -0.1, 0.2, -0.2],
            sample_rate: 44100,
        })
        .unwrap();

    // Session should still be in AwaitingCardExchange (proximity check done)
    assert!(
        matches!(alice.state(), ExchangeState::AwaitingCardExchange { .. }),
        "after audio response, should be in AwaitingCardExchange, got {:?}",
        alice.state()
    );
}

// −− Audio unavailable doesn't block exchange −−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn audio_hardware_unavailable_does_not_block_key_agreement() {
    let (mut alice, _bob) = create_peer_scanned_sessions();

    alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    let _ = alice.drain_commands();

    // Audio unavailable — should not fail the session
    alice
        .apply_hardware_event(Event::HardwareUnavailable {
            transport: "Audio".into(),
        })
        .unwrap();

    // Session should still be usable (AwaitingCardExchange)
    assert!(
        matches!(alice.state(), ExchangeState::AwaitingCardExchange { .. }),
        "audio unavailable should not break the session"
    );
}
