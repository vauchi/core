// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::ui::AppEngine;
use vauchi_core::exchange::DeviceRequirement;
use vauchi_core::exchange::capability::PermissionState;
use vauchi_core::{AlertSpec, Command, Event, Vauchi};

const GUIDANCE: &str = "This system could not open a permission prompt. Check the app's access in system settings, then try again. On a managed device, ask your administrator for help.";
fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.initial_commands().unwrap();
    engine
}
fn required(engine: &mut AppEngine, transport: &str) -> Vec<Command> {
    engine
        .dispatch(Event::PermissionRequired {
            transport: transport.into(),
        })
        .unwrap()
}
fn alert(title: &str) -> Command {
    Command::PresentAlert {
        alert: AlertSpec {
            title: title.into(),
            message: GUIDANCE.into(),
        },
    }
}

// @internal — lack of an OS prompt must not grant permission or hide the transport.
#[test]
fn unavailable_permission_prompt_shows_settings_guidance_without_granting() {
    let mut engine = engine();
    let before = engine
        .transport_readiness()
        .permission(DeviceRequirement::Ble);
    assert_eq!(
        required(&mut engine, "ble"),
        vec![Command::RequestPermission {
            transport: "ble".into()
        }]
    );
    let commands = engine
        .dispatch(Event::HardwareUnavailable {
            transport: "ble".into(),
        })
        .unwrap();
    assert!(commands.contains(&alert("Bluetooth error")), "{commands:?}");
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        before
    );
    assert_eq!(
        required(&mut engine, "ble"),
        vec![Command::RequestPermission {
            transport: "ble".into()
        }]
    );
}

// @internal — an unresolved native permission request is unique per capability.
#[test]
fn permission_requests_coalesce_until_the_original_request_finishes() {
    let mut engine = engine();
    assert_eq!(
        required(&mut engine, "BLE"),
        vec![Command::RequestPermission {
            transport: "BLE".into()
        }]
    );
    assert_eq!(required(&mut engine, "ble"), Vec::new());
    engine
        .dispatch(Event::PermissionDenied {
            transport: "BLE".into(),
        })
        .unwrap();
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        PermissionState::Denied
    );
    assert_eq!(
        required(&mut engine, "ble"),
        vec![Command::RequestPermission {
            transport: "ble".into()
        }]
    );
}

// @internal — permission failure copy must not expose unknown capability names or OS details.
#[test]
fn unknown_permission_and_failed_native_request_use_prepared_copy() {
    let mut engine = engine();
    assert_eq!(
        required(&mut engine, "<b>private</b>"),
        vec![alert("Hardware error")]
    );
    required(&mut engine, "camera");
    let commands = engine
        .dispatch(Event::HardwareError {
            transport: "camera".into(),
            error: "secret native path /home/private".into(),
        })
        .unwrap();
    assert!(commands.contains(&alert("Camera error")), "{commands:?}");
    assert!(!format!("{commands:?}").contains("/home/private"));
}

// @internal — later hardware failures must not be mistaken for completed permission prompts.
#[test]
fn granted_and_denied_requests_clear_pending_prompt_state() {
    for outcome in [
        Event::PermissionGranted {
            transport: "camera".into(),
        },
        Event::PermissionDenied {
            transport: "camera".into(),
        },
    ] {
        let mut engine = engine();
        required(&mut engine, "camera");
        engine.dispatch(outcome).unwrap();
        let commands = engine
            .dispatch(Event::HardwareUnavailable {
                transport: "camera".into(),
            })
            .unwrap();
        assert!(!commands.contains(&alert("Camera error")), "{commands:?}");
        assert_eq!(
            required(&mut engine, "camera"),
            vec![Command::RequestPermission {
                transport: "camera".into(),
            }]
        );
    }
}

// @internal — wiping the application cancels outstanding permission requests.
#[test]
fn application_reset_allows_a_fresh_permission_request() {
    let mut engine = engine();
    required(&mut engine, "camera");
    assert_eq!(
        engine.route_to_shells(vec![Command::ResetApplication]),
        vec![Command::ResetApplication]
    );
    assert_eq!(
        required(&mut engine, "camera"),
        vec![Command::RequestPermission {
            transport: "camera".into()
        }]
    );
}

// @internal — an unsupported permission must not swallow other pending display commands.
#[test]
fn unknown_permission_preserves_other_pending_commands() {
    let mut engine = engine();
    engine.extend_pending_commands([Command::ReloadTheme]);
    assert_eq!(
        required(&mut engine, "unknown"),
        vec![Command::ReloadTheme, alert("Hardware error")]
    );
}
