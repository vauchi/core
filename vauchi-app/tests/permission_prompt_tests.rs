// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::exchange::DeviceRequirement;
use vauchi_core::exchange::capability::PermissionState;
use vauchi_core::{AlertSpec, Command, Event, InteractionId, PresentationNode, Vauchi};

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

fn camera_action(nodes: &[PresentationNode]) -> Option<InteractionId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Group { children, .. } => camera_action(children),
        PresentationNode::List { rows, .. } => rows
            .iter()
            .find(|row| row.title == "Glance")
            .and_then(|row| {
                row.activation
                    .as_ref()
                    .map(|action| action.interaction_id.clone())
            }),
        _ => None,
    })
}
fn activate_camera_row(commands: &[Command]) -> Event {
    commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => {
                camera_action(&surface.nodes).map(|interaction_id| Event::ActionActivated {
                    surface_id: surface.surface_id.clone(),
                    interaction_id,
                })
            }
            _ => None,
        })
        .expect("the denied picker must offer the Glance permission action")
}

// @internal — a prepared picker tap requests OS access and never grants it itself.
#[test]
fn picker_permission_tap_waits_for_the_os_result() {
    let mut engine = engine();
    engine.set_device_capabilities(vauchi_core::exchange_types::DeviceCapabilities {
        has_camera: true,
        has_ble: true,
        ..Default::default()
    });
    engine.navigate_to(AppScreen::Exchange);
    engine.initial_commands().unwrap();
    let denied = engine
        .dispatch(Event::PermissionDenied {
            transport: "camera".into(),
        })
        .unwrap();
    let requested = engine.dispatch(activate_camera_row(&denied)).unwrap();
    assert!(requested.contains(&Command::RequestPermission {
        transport: "camera".into()
    }));
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Denied
    );
    assert_eq!(
        requested
            .iter()
            .filter(|command| matches!(command, Command::CameraCapture { .. }))
            .count(),
        0
    );
    let retry = activate_camera_row(&requested);
    let unavailable = engine
        .dispatch(Event::HardwareUnavailable {
            transport: "camera".into(),
        })
        .unwrap();
    assert!(unavailable.contains(&alert("Camera error")));
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Denied
    );
    assert!(
        engine
            .dispatch(retry)
            .unwrap()
            .contains(&Command::RequestPermission {
                transport: "camera".into()
            })
    );
    engine
        .dispatch(Event::PermissionGranted {
            transport: "camera".into(),
        })
        .unwrap();
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Granted
    );
}
