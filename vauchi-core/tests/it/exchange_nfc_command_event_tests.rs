// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the NFC exchange flow via the ADR-031 command/event protocol.
//!
//! NFC exchange uses a single tap to exchange key material. The protocol
//! is simpler than BLE: both sides present their NFC payload, the OS
//! handles the tap, and core processes the received data.

use vauchi_core::ContactCard;
use vauchi_core::exchange::{
    ExchangeCommand, ExchangeHardwareEvent, ExchangeSession, ExchangeState,
    ManualConfirmationVerifier,
};
use vauchi_core::identity::Identity;

/// Helper: create an NFC exchange session.
fn nfc_session(name: &str) -> ExchangeSession {
    let identity = Identity::create(name);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    ExchangeSession::new_nfc(identity, card, proximity)
}

// −− Initial commands −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn nfc_session_emits_activate_with_payload() {
    let mut session = nfc_session("Alice");
    session.emit_initial_commands();
    let cmds = session.drain_commands();

    assert_eq!(cmds.len(), 1, "expected exactly one NfcActivate command");

    match &cmds[0] {
        ExchangeCommand::NfcActivate { payload } => {
            assert!(
                !payload.is_empty(),
                "NfcActivate payload should contain our NFC exchange data, got empty"
            );
        }
        other => panic!("expected NfcActivate, got {:?}", other),
    }
}

// −− NFC tap completes exchange −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn nfc_data_received_with_valid_payload_completes_exchange() {
    let mut alice = nfc_session("Alice");
    let mut bob = nfc_session("Bob");

    // Get Bob's NFC payload from his initial commands
    bob.emit_initial_commands();
    let bob_cmds = bob.drain_commands();
    let bob_payload = match &bob_cmds[0] {
        ExchangeCommand::NfcActivate { payload } => payload.clone(),
        _ => panic!("expected NfcActivate"),
    };

    // Alice receives Bob's NFC payload via tap
    alice
        .apply_hardware_event(ExchangeHardwareEvent::NfcDataReceived { data: bob_payload })
        .unwrap();

    // After NFC tap, session should reach AwaitingKeyAgreement or beyond
    // The NFC tap is the proximity proof — key agreement should auto-proceed
    assert!(
        !matches!(alice.state(), ExchangeState::Failed { .. }),
        "NFC tap with valid payload should not fail, got {:?}",
        alice.state()
    );
    assert!(
        !matches!(alice.state(), ExchangeState::AwaitingNfcTap),
        "After NFC tap, should advance beyond AwaitingNfcTap"
    );
}

// −− NFC hardware unavailable −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn nfc_hardware_unavailable_does_not_crash() {
    let mut session = nfc_session("Alice");

    // NFC unavailable should not fail the session fatally
    session
        .apply_hardware_event(ExchangeHardwareEvent::HardwareUnavailable {
            transport: "NFC".into(),
        })
        .unwrap();

    // Session should still be in AwaitingNfcTap (not failed)
    assert!(
        matches!(session.state(), ExchangeState::AwaitingNfcTap),
        "HardwareUnavailable should not change NFC state"
    );
}

// −− NFC deactivate on completion −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn nfc_tap_emits_deactivate_after_processing() {
    let mut alice = nfc_session("Alice");
    let mut bob = nfc_session("Bob");

    bob.emit_initial_commands();
    let bob_cmds = bob.drain_commands();
    let bob_payload = match &bob_cmds[0] {
        ExchangeCommand::NfcActivate { payload } => payload.clone(),
        _ => panic!("expected NfcActivate"),
    };

    alice
        .apply_hardware_event(ExchangeHardwareEvent::NfcDataReceived { data: bob_payload })
        .unwrap();

    // After processing, NFC interface should be deactivated
    let cmds = alice.drain_commands();
    let has_deactivate = cmds
        .iter()
        .any(|c| matches!(c, ExchangeCommand::NfcDeactivate));
    assert!(
        has_deactivate,
        "after NFC tap processing, should emit NfcDeactivate, got {:?}",
        cmds
    );
}
