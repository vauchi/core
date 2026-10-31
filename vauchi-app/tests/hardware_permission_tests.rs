// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "testing")]

//! A hardware shell shows no UI (ADR-077): when the OS needs the person's
//! permission it reports so, Core has the display shell ask, and the
//! display shell reports the answer.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::AppEngine;
use vauchi_core::clock::FakeClock;
use vauchi_core::exchange::DeviceRequirement;
use vauchi_core::exchange::capability::PermissionState;
use vauchi_core::{Command, CommandTarget, Event, EventJsonError, Vauchi, event_from_json};

fn engine() -> AppEngine {
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut vauchi = Vauchi::in_memory_with_clock(clock).unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn attached() -> AppEngine {
    let mut engine = engine();
    engine
        .dispatch(
            event_from_json(
                r#"{"HardwareCapabilitiesReported":{"capabilities":{"has_ble":true}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    engine
}

fn ble(event: fn(String) -> Event) -> Event {
    event("ble".into())
}

// @internal — the prompt is display work, even with a hardware shell attached.
#[test]
fn a_required_permission_makes_the_display_shell_ask() {
    let mut engine = attached();

    let batch = engine
        .dispatch(ble(|transport| Event::PermissionRequired { transport }))
        .unwrap();

    let request = Command::RequestPermission {
        transport: "ble".into(),
    };
    assert!(batch.contains(&request), "display batch carried {batch:?}");
    assert_eq!(request.target(), CommandTarget::Display);
    assert_eq!(engine.drain_hardware_commands(), Vec::new());
}

// @internal — the display shell's answer reaches the readiness ledger.
#[test]
fn a_granted_permission_clears_an_earlier_denial() {
    let mut engine = attached();
    engine
        .dispatch(ble(|transport| Event::PermissionDenied { transport }))
        .unwrap();
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        PermissionState::Denied
    );

    engine
        .dispatch(ble(|transport| Event::PermissionGranted { transport }))
        .unwrap();

    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        PermissionState::Granted
    );
}

// @internal — DC-01: the transport name is bounded at the parse boundary.
#[test]
fn an_oversized_transport_name_is_refused_at_the_boundary() {
    for event in ["PermissionRequired", "PermissionGranted"] {
        let json = |name: &str| serde_json::json!({event: {"transport": name}}).to_string();
        assert_eq!(
            event_from_json(&json(&"t".repeat(65))),
            Err(EventJsonError::Malformed),
            "{event}"
        );
        assert!(event_from_json(&json(&"t".repeat(64))).is_ok(), "{event}");
    }
}
