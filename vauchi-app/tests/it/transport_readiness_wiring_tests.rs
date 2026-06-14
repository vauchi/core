// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine wiring for the TransportReadiness ledger (Phase 2 T2.1b of
//! `2026-06-11-exchange-waits-forever-without-capabilities`).
//!
//! `Event::PermissionDenied` at the `handle_hardware_event` choke point feeds
//! the device-wide ledger up front (regardless of screen). Non-transport
//! labels — notably `"location"` (the ADR-051 capture-geolocation permission)
//! — are ignored, so declining a capture prompt never gates a BLE mode.

use vauchi_app::ui::AppEngine;
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::PermissionState;
use vauchi_core::exchange::mode::DeviceRequirement;

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn permission_denied_event_updates_the_readiness_ledger() {
    let mut engine = engine();
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Unknown,
        "ledger starts clean"
    );

    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });

    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Denied,
        "a camera PermissionDenied must be recorded device-wide"
    );
}

// @internal
#[test]
fn location_permission_denied_does_not_touch_any_transport() {
    // "location" is the ADR-051 capture-geolocation permission, not a
    // transport — it must not deny BLE (or anything) in the ledger.
    let mut engine = engine();

    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "location".into(),
    });

    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        PermissionState::Unknown,
        "a location denial is the capture-geolocation permission, not BLE"
    );
}

// @internal
#[test]
fn ble_permission_denied_event_updates_the_ledger() {
    // iOS emits "BLE" (uppercase); exercise the real label through the
    // AppEngine choke point, not just the unit mapping.
    let mut engine = engine();

    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "BLE".into(),
    });

    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        PermissionState::Denied
    );
}
