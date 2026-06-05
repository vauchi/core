// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level integration tests for the graduated BLE exchange engine.
//!
//! Slice 4 of `2026-05-11-ble-exchange-engine-graduation`. Pins the routing
//! seam added in slice 2: picking a BLE mode (`Magic` / `Bump` / `Shake`) on
//! the mode picker emits `ActionResult::StartBleExchange`, the AppEngine
//! navigates to `AppScreen::BleExchange`, and the live `BleExchangeEngine`
//! drives the flow through the uniform AppEngine surface
//! (`current_screen` / `handle_action` / `handle_hardware_event`).
//!
//! Mirrors `exchange_cancel_navigation_tests.rs` (the multi-stage sibling).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// Enter the exchange flow and pick `item_id` (e.g. `mode:magic`). With no
/// groups the picker hands off directly. Returns the engine parked on the
/// resulting screen.
fn enter_ble_mode(item_id: &str) -> AppEngine {
    let mut engine = engine_with_identity();
    let entry = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(entry.screen_id, "exchange_mode_selection");
    // The picker parses only the `mode:` prefix of `item_id`; the
    // component_id (category) is ignored.
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: item_id.into(),
    });
    engine
}

// @internal
#[test]
fn selecting_magic_routes_to_ble_exchange_and_renders_discovering() {
    let engine = enter_ble_mode("mode:magic");
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::BleExchange {
                mode: vauchi_core::exchange::mode::ExchangeMode::Magic
            }
        ),
        "picking Magic should navigate to BleExchange, got {:?}",
        engine.current_app_screen()
    );
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering",
        "the live BleExchangeEngine should render the discovering screen"
    );
}

// @internal
#[test]
fn bump_and_shake_also_route_to_ble_exchange() {
    for (item, mode) in [
        ("mode:bump", vauchi_core::exchange::mode::ExchangeMode::Bump),
        (
            "mode:shake",
            vauchi_core::exchange::mode::ExchangeMode::Shake,
        ),
    ] {
        let engine = enter_ble_mode(item);
        match engine.current_app_screen() {
            AppScreen::BleExchange { mode: got } => assert_eq!(
                *got, mode,
                "{item} should route to BleExchange with mode {mode:?}"
            ),
            other => panic!("{item} should route to BleExchange, got {other:?}"),
        }
    }
}

// @internal
#[test]
fn forwarding_ble_discovery_advances_engine_to_exchanging() {
    let mut engine = enter_ble_mode("mode:magic");
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering"
    );

    // A discovered peer drives Discovering → Handshaking, which renders the
    // shared exchanging screen and emits a connect command.
    let result = engine.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "peer-1".into(),
        rssi: -45,
        adv_data: vec![],
    });
    match result {
        Some(ActionResult::Commands { commands }) => assert!(
            matches!(&commands[0], vauchi_core::Command::BleConnect { device_id } if device_id == "peer-1"),
            "discovery should emit BleConnect, got {commands:?}"
        ),
        other => panic!("expected Commands from the active BLE engine, got {other:?}"),
    }
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_exchanging",
        "after discovery the engine should render the exchanging screen"
    );
}

// @internal
#[test]
fn ble_disconnect_forwarded_through_app_engine_renders_failed() {
    let mut engine = enter_ble_mode("mode:magic");
    let _ = engine.handle_hardware_event(Event::BleDisconnected {
        reason: "peer hung up".into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_failed",
        "a forwarded disconnect should flip the engine to the failed screen"
    );
}

// @internal
#[test]
fn cancel_on_ble_exchange_lands_on_mode_picker() {
    let mut engine = enter_ble_mode("mode:magic");
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::BleExchange { .. }
    ));

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    let landed = match &result {
        ActionResult::NavigateTo(screen) => screen.screen_id.clone(),
        other => panic!("expected NavigateTo from cancel completion, got {other:?}"),
    };
    assert!(
        !landed.is_empty(),
        "Cancel must not land on an empty screen_id (white-screen regression)"
    );
    assert_eq!(
        landed, "exchange_mode_selection",
        "Cancel should return to the re-armed mode picker"
    );
    assert!(
        !matches!(engine.current_app_screen(), AppScreen::BleExchange { .. }),
        "Cancel must navigate off the BleExchange AppScreen, still on {:?}",
        engine.current_app_screen()
    );
}
