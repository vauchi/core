// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tests for transport fallback via DeviceCapabilities (ADR-031).
//!
//! When a transport reports HardwareUnavailable, the session should
//! fall back to the next available transport based on DeviceCapabilities.

use vauchi_core::ContactCard;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::{ExchangeSession, ManualConfirmationVerifier};
use vauchi_core::identity::Identity;
use vauchi_core::{Command, Event};

fn nfc_session_with_caps(name: &str, caps: DeviceCapabilities) -> ExchangeSession {
    let identity = Identity::create(name, 0);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    let mut session = ExchangeSession::new_nfc(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    session.set_device_capabilities(caps);
    session
}

// −− Fallback from NFC -> QR −−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn nfc_unavailable_with_camera_falls_back_to_qr() {
    let caps = DeviceCapabilities {
        has_nfc: true,
        has_camera: true,
        ..Default::default()
    };
    let mut session = nfc_session_with_caps("Alice", caps);
    session.emit_initial_commands();
    let _ = session.drain_commands();

    session
        .apply_hardware_event(Event::HardwareUnavailable {
            transport: "NFC".into(),
        })
        .unwrap();

    let cmds = session.drain_commands();
    let has_qr = cmds.iter().any(|c| matches!(c, Command::QrDisplay { .. }));
    assert!(
        has_qr,
        "NFC unavailable with camera should fall back to QR, got {:?}",
        cmds
    );
}

// −− No fallback when no alternatives available −−−−−−−−−−−−−−−−−−−−−−

// @internal
#[test]
fn nfc_unavailable_without_camera_does_not_fall_back() {
    let caps = DeviceCapabilities {
        has_nfc: true,
        has_camera: false,
        has_ble: false,
        ..Default::default()
    };
    let mut session = nfc_session_with_caps("Alice", caps);
    session.emit_initial_commands();
    let _ = session.drain_commands();

    session
        .apply_hardware_event(Event::HardwareUnavailable {
            transport: "NFC".into(),
        })
        .unwrap();

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
    let caps = DeviceCapabilities {
        has_nfc: true,
        has_camera: true,
        has_ble: false,
        ..Default::default()
    };
    let session = nfc_session_with_caps("Alice", caps);
    assert!(
        matches!(
            session.state(),
            vauchi_core::exchange::ExchangeState::AwaitingNfcTap
        ),
        "session should remain in AwaitingNfcTap after setting capabilities"
    );
}

// A denied permission leaves the transport as unusable as missing hardware.
// @internal
#[test]
fn nfc_permission_denied_with_camera_falls_back_to_qr() {
    let caps = DeviceCapabilities {
        has_nfc: true,
        has_camera: true,
        ..Default::default()
    };
    let mut session = nfc_session_with_caps("Alice", caps);
    session.emit_initial_commands();
    let _ = session.drain_commands();

    session
        .apply_hardware_event(Event::PermissionDenied {
            transport: "NFC".into(),
        })
        .unwrap();

    let cmds = session.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(c, Command::QrDisplay { .. })),
        "{cmds:?}"
    );
}
