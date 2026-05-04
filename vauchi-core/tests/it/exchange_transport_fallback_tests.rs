// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for transport fallback via DeviceCapabilities (ADR-031).
//!
//! When a transport reports HardwareUnavailable, the session should
//! fall back to the next available transport based on DeviceCapabilities.

use vauchi_core::ContactCard;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::{ExchangeSession, ManualConfirmationVerifier};
use vauchi_core::identity::Identity;
use vauchi_core::{Command, Event};

fn ble_session_with_caps(name: &str, caps: DeviceCapabilities) -> ExchangeSession {
    let identity = Identity::create(name);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    let mut session = ExchangeSession::new_ble(identity, card, proximity);
    session.set_device_capabilities(caps);
    session
}

// −− Fallback from BLE -> QR −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn ble_unavailable_with_camera_falls_back_to_qr() {
    let caps = DeviceCapabilities {
        has_ble: true,
        has_camera: true,
        ..Default::default()
    };
    let mut session = ble_session_with_caps("Alice", caps);
    session.emit_initial_commands();
    let _ = session.drain_commands(); // drain initial BLE commands

    // BLE hardware reports unavailable
    session
        .apply_hardware_event(Event::HardwareUnavailable {
            transport: "BLE".into(),
        })
        .unwrap();

    // Should fall back to QR — emit QrDisplay command
    let cmds = session.drain_commands();
    let has_qr = cmds.iter().any(|c| matches!(c, Command::QrDisplay { .. }));
    assert!(
        has_qr,
        "BLE unavailable with camera should fall back to QR, got {:?}",
        cmds
    );
}

// −− No fallback when no alternatives available −−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn ble_unavailable_without_camera_does_not_fall_back() {
    let caps = DeviceCapabilities {
        has_ble: true,
        has_camera: false,
        has_nfc: false,
        ..Default::default()
    };
    let mut session = ble_session_with_caps("Alice", caps);
    session.emit_initial_commands();
    let _ = session.drain_commands();

    session
        .apply_hardware_event(Event::HardwareUnavailable {
            transport: "BLE".into(),
        })
        .unwrap();

    // No fallback — session should not emit new transport commands
    let cmds = session.drain_commands();
    assert!(
        cmds.is_empty(),
        "no fallback available, should not emit commands, got {:?}",
        cmds
    );
}

// −− DeviceCapabilities setter −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn set_device_capabilities_is_accessible() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = ManualConfirmationVerifier::new();
    let mut session = ExchangeSession::new_ble(identity, card, proximity);

    let caps = DeviceCapabilities {
        has_ble: true,
        has_camera: true,
        has_nfc: false,
        ..Default::default()
    };
    session.set_device_capabilities(caps);
    // Verify session is still usable after setting capabilities
    assert!(
        matches!(
            session.state(),
            vauchi_core::exchange::ExchangeState::AwaitingBleConnection
        ),
        "session should remain in AwaitingBleConnection after setting capabilities"
    );
}
