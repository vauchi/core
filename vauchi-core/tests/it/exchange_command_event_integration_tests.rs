// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the full ADR-031 command/event exchange protocol.
//!
//! These tests verify complete exchange flows (QR, BLE, NFC) using only
//! `Command`s and `Event`s — no hardware traits,
//! no mock transports, no blocking calls.

use vauchi_core::ContactCard;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeSession, ExchangeState, ManualConfirmationVerifier,
};
use vauchi_core::identity::Identity;
use vauchi_core::{Command, Event};

// −− Helpers −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

fn qr_session(name: &str) -> ExchangeSession {
    let identity = Identity::create(name, 0);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    )
}

fn nfc_session(name: &str) -> ExchangeSession {
    let identity = Identity::create(name, 0);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    ExchangeSession::new_nfc(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    )
}

fn ble_session(name: &str) -> ExchangeSession {
    let identity = Identity::create(name, 0);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    ExchangeSession::new_ble(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    )
}

// −− QR full round-trip −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn qr_full_round_trip_via_commands_and_events() {
    let mut alice = qr_session("Alice");
    let mut bob = qr_session("Bob");

    // Both start QR and emit initial commands
    alice.apply(ExchangeEvent::StartQR).unwrap();
    alice.emit_initial_commands();
    bob.apply(ExchangeEvent::StartQR).unwrap();
    bob.emit_initial_commands();

    // Get QR data from display commands
    let alice_cmds = alice.drain_commands();
    let bob_cmds = bob.drain_commands();

    let alice_qr_data = alice_cmds
        .iter()
        .find_map(|c| match c {
            Command::QrDisplay { data } => Some(data.clone()),
            _ => None,
        })
        .expect("Alice should emit QrDisplay");

    let bob_qr_data = bob_cmds
        .iter()
        .find_map(|c| match c {
            Command::QrDisplay { data } => Some(data.clone()),
            _ => None,
        })
        .expect("Bob should emit QrDisplay");

    // Alice scans Bob's QR
    alice
        .apply_hardware_event(Event::QrScanned { data: bob_qr_data })
        .unwrap();

    // Bob scans Alice's QR
    bob.apply_hardware_event(Event::QrScanned {
        data: alice_qr_data,
    })
    .unwrap();

    // Both should be in PeerScanned state
    assert!(
        matches!(alice.state(), ExchangeState::PeerScanned { .. }),
        "Alice should be PeerScanned, got {:?}",
        alice.state()
    );
    assert!(
        matches!(bob.state(), ExchangeState::PeerScanned { .. }),
        "Bob should be PeerScanned, got {:?}",
        bob.state()
    );
}

// −− NFC full round-trip −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn nfc_full_round_trip_via_commands_and_events() {
    let mut alice = nfc_session("Alice");
    let mut bob = nfc_session("Bob");

    // Both emit initial NfcActivate with payload
    alice.emit_initial_commands();
    bob.emit_initial_commands();

    let alice_payload = alice
        .drain_commands()
        .into_iter()
        .find_map(|c| match c {
            Command::NfcActivate { payload } => Some(payload),
            _ => None,
        })
        .expect("Alice should emit NfcActivate with payload");

    let bob_payload = bob
        .drain_commands()
        .into_iter()
        .find_map(|c| match c {
            Command::NfcActivate { payload } => Some(payload),
            _ => None,
        })
        .expect("Bob should emit NfcActivate with payload");

    assert!(
        !alice_payload.is_empty(),
        "Alice NFC payload should not be empty"
    );
    assert!(
        !bob_payload.is_empty(),
        "Bob NFC payload should not be empty"
    );

    // Alice taps Bob (receives Bob's payload)
    alice
        .apply_hardware_event(Event::NfcDataReceived { data: bob_payload })
        .unwrap();

    // Bob taps Alice (receives Alice's payload)
    bob.apply_hardware_event(Event::NfcDataReceived {
        data: alice_payload,
    })
    .unwrap();

    // Both should have advanced past AwaitingNfcTap
    assert!(
        !matches!(alice.state(), ExchangeState::AwaitingNfcTap),
        "Alice should advance past AwaitingNfcTap"
    );
    assert!(
        !matches!(bob.state(), ExchangeState::AwaitingNfcTap),
        "Bob should advance past AwaitingNfcTap"
    );

    // Both should emit NfcDeactivate
    let alice_cmds = alice.drain_commands();
    let bob_cmds = bob.drain_commands();
    assert!(
        alice_cmds
            .iter()
            .any(|c| matches!(c, Command::NfcDeactivate)),
        "Alice should emit NfcDeactivate after tap"
    );
    assert!(
        bob_cmds.iter().any(|c| matches!(c, Command::NfcDeactivate)),
        "Bob should emit NfcDeactivate after tap"
    );
}

// −− BLE -> QR fallback −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn ble_to_qr_fallback_produces_working_qr_session() {
    let mut session = ble_session("Alice");
    session.set_device_capabilities(DeviceCapabilities {
        has_ble: true,
        has_camera: true,
        ..Default::default()
    });
    session.emit_initial_commands();
    let _ = session.drain_commands(); // drain BLE commands

    // BLE unavailable -> should fall back to QR
    session
        .apply_hardware_event(Event::HardwareUnavailable {
            transport: "BLE".into(),
        })
        .unwrap();

    let cmds = session.drain_commands();
    let qr_data = cmds.iter().find_map(|c| match c {
        Command::QrDisplay { data } => Some(data.clone()),
        _ => None,
    });
    assert!(
        qr_data.is_some(),
        "BLE fallback should produce QrDisplay command"
    );

    // The QR data should be scannable
    let qr_str = qr_data.unwrap();
    assert!(!qr_str.is_empty(), "QR data should not be empty");

    // Another session should be able to scan this QR
    let mut bob = qr_session("Bob");
    bob.apply(ExchangeEvent::StartQR).unwrap();
    bob.apply_hardware_event(Event::QrScanned { data: qr_str })
        .unwrap();
    assert!(
        matches!(bob.state(), ExchangeState::PeerScanned { .. }),
        "Bob should be able to scan Alice's fallback QR"
    );
}

// −− Mixed transport: command/event isolation −−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn qr_session_ignores_ble_events() {
    let mut session = qr_session("Alice");
    session.apply(ExchangeEvent::StartQR).unwrap();

    // BLE events on a QR session should not crash or change state
    session
        .apply_hardware_event(Event::BleDeviceDiscovered {
            id: "rogue".into(),
            rssi: -80,
            adv_data: vec![],
        })
        .unwrap();

    // Should still be in DisplayingQr
    assert!(
        matches!(session.state(), ExchangeState::DisplayingQr { .. }),
        "QR session should ignore BLE events"
    );
}

// @internal
#[test]
fn nfc_session_ignores_ble_events() {
    let mut session = nfc_session("Alice");

    session
        .apply_hardware_event(Event::BleConnected {
            device_id: "rogue".into(),
        })
        .unwrap();

    assert!(
        matches!(session.state(), ExchangeState::AwaitingNfcTap),
        "NFC session should ignore BLE events"
    );
}

// −− Location seam (ADR-051 contact annotations, T2.1) −−−−−−−−−−−−−−−−−−−−−−−−−

// @scenario: contact-annotations.feature - Exchange captures coordinates
// @internal
#[test]
fn location_command_and_event_round_trip_and_name() {
    // variant_name is payload-free and stable for diagnostics.
    assert_eq!(
        Command::LocationRequest { timeout_ms: 5000 }.variant_name(),
        "LocationRequest"
    );

    // Both cross the wire via serde — round-trip preserves fields.
    let cmd = Command::LocationRequest { timeout_ms: 8000 };
    let cmd_back: Command = serde_json::from_str(&serde_json::to_string(&cmd).unwrap()).unwrap();
    assert_eq!(cmd, cmd_back);

    let evt = Event::LocationResult {
        latitude: 52.5200,
        longitude: 13.4050,
        accuracy_meters: Some(12.5),
    };
    let evt_back: Event = serde_json::from_str(&serde_json::to_string(&evt).unwrap()).unwrap();
    assert_eq!(evt, evt_back);
}

// @scenario: contact-annotations.feature - Exchange captures coordinates
// @internal
#[test]
fn exchange_session_ignores_location_result() {
    let mut session = qr_session("Alice");
    let before = format!("{:?}", session.state());

    // A location fix is an annotation, not a handshake event — the state
    // machine must accept it without erroring or changing state.
    session
        .apply_hardware_event(Event::LocationResult {
            latitude: 1.0,
            longitude: 2.0,
            accuracy_meters: None,
        })
        .expect("LocationResult must be accepted by the session");

    assert_eq!(
        before,
        format!("{:?}", session.state()),
        "LocationResult must not change exchange state"
    );
}
