// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end Glance exchange where the scanner has no camera and enters
//! the displayer's code as text (`2026-09-09-tui-cannot-ingest-peer-
//! exchange-payload`). Mirrors the in-src
//! `glance_orchestration_symmetric_happy_path_both_persist`, but drives the
//! scanner through the uniform `AppEngine` surface — screen, text input,
//! action — instead of calling `apply_glance_scan` directly.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, GLANCE_CODE_INPUT_ID, QrMode, UserAction,
    WorkflowEngine,
};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::platform::BleLinkDirection;

fn engine_named(name: &str, has_camera: bool) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity(name).expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_ble: true,
        has_camera,
        ..Default::default()
    });
    engine
}

fn open_glance(engine: &mut AppEngine) {
    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.navigate_to(AppScreen::BleExchange {
        mode: ExchangeMode::Glance,
    });
}

fn displayed_code(engine: &AppEngine) -> String {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::QrCode {
                data,
                mode: QrMode::Display,
                ..
            } => Some(data.clone()),
            _ => None,
        })
        .expect("the glance screen displays this device's code")
}

fn signing_key(engine: &AppEngine) -> [u8; 32] {
    *engine
        .vauchi()
        .identity()
        .expect("identity")
        .signing_public_key()
}

fn enter_code(engine: &mut AppEngine, code: &str) -> ActionResult {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: GLANCE_CODE_INPUT_ID.into(),
        value: code.into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "connect_code".into(),
    })
}

/// Route one side's pending BLE writes to the other as notifications.
fn pump(from: &mut AppEngine, to: &mut AppEngine) -> usize {
    let mut routed = 0;
    for cmd in from.drain_pending_commands() {
        if let vauchi_core::Command::BleWriteCharacteristic { uuid, data, .. } = cmd {
            routed += 1;
            let ev = to.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                device_id: String::new(),
                direction: BleLinkDirection::Outbound,
                uuid,
                data,
            });
            to.apply_ble_machine_event(ev);
        }
    }
    routed
}

fn run_handshake(initiator: &mut AppEngine, responder: &mut AppEngine) {
    let ei = initiator.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "responder".into(),
        direction: BleLinkDirection::Outbound,
    });
    initiator.apply_ble_machine_event(ei);
    let er = responder.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "initiator".into(),
        direction: BleLinkDirection::Inbound,
    });
    responder.apply_ble_machine_event(er);
    for _ in 0..50 {
        let a = pump(initiator, responder);
        let b = pump(responder, initiator);
        if a + b == 0 {
            break;
        }
    }
}

// @scenario: contact_exchange.feature :: Glance without a camera accepts the peer code as text
#[test]
fn camera_less_scanner_completes_glance_by_entering_the_displayed_code() {
    let mut alice = engine_named("Alice", true);
    let mut bob = engine_named("Bob", false);
    open_glance(&mut alice);
    open_glance(&mut bob);
    let alice_code = displayed_code(&alice);

    let result = enter_code(&mut bob, &alice_code);

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("an accepted code waits for discovery, got {result:?}");
    };
    assert_eq!(screen.screen_id, "exchange_ble_discovering");

    bob.handle_glance_discovery("alice-device", &signing_key(&alice));
    assert!(
        bob.ble_handshake_session_active(),
        "the typed code must pin Alice exactly as a camera scan would"
    );
    let connects = bob
        .drain_pending_commands()
        .into_iter()
        .filter(|c| matches!(c, vauchi_core::Command::BleConnect { device_id } if device_id == "alice-device"))
        .count();
    assert_eq!(connects, 1, "bob dials the pinned displayer once");

    alice.start_ble_handshake_as_responder();
    run_handshake(&mut bob, &mut alice);

    let bob_contacts = bob.vauchi().list_contacts().expect("contacts");
    let alice_contacts = alice.vauchi().list_contacts().expect("contacts");
    assert_eq!(bob_contacts.len(), 1);
    assert_eq!(bob_contacts[0].display_name(), "Alice");
    assert_eq!(alice_contacts.len(), 1);
    assert_eq!(alice_contacts[0].display_name(), "Bob");
}

// @scenario: contact_exchange.feature :: A malformed peer code fails to the retry screen
#[test]
fn garbage_code_fails_to_retry_and_pins_nobody() {
    let alice = engine_named("Alice", true);
    let mut bob = engine_named("Bob", false);
    open_glance(&mut bob);

    let result = enter_code(&mut bob, "definitely not a code");

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("garbage must render the failed screen, got {result:?}");
    };
    assert_eq!(screen.screen_id, "exchange_failed");
    assert!(
        screen.contextual_actions.iter().any(|a| a.id == "retry"),
        "the failed screen offers retry"
    );

    bob.handle_glance_discovery("alice-device", &signing_key(&alice));
    assert!(
        !bob.ble_handshake_session_active(),
        "a rejected code must not pin any peer"
    );
}
