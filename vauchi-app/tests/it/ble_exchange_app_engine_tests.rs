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

#[cfg(feature = "testing")]
use std::sync::Arc;
#[cfg(feature = "testing")]
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
// FakeClock is `#[cfg(any(test, feature = "testing"))]`; the no-feature
// pre-push compile check excludes the clock-driven test below with it.
#[cfg(feature = "testing")]
use vauchi_core::clock::{Clock, FakeClock};

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
    // Mode-selection root reports the canonical tab-root id `exchange`
    // (so frontends render the bottom nav bar) — see canonical_screen_id_tests.
    assert_eq!(entry.screen_id, "exchange");
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
    // Peer advertises a token that sorts above this device's 32-byte
    // identity token (0xFF * 33 ≥ any 32-byte value), so this device wins
    // the tiebreak and initiates the connection.
    let result = engine.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "peer-1".into(),
        rssi: -45,
        adv_data: vec![0xFF; 33],
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
        device_id: "peer-1".into(),
        direction: vauchi_core::BleLinkDirection::Outbound,
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
fn ble_permission_denied_mid_wait_renders_failed_screen() {
    // T2.3 mid-session: a BLE permission revoked WHILE the engine is in the
    // live wait must surface the SAME `exchange_failed` retry/cancel screen the
    // pre-entry guard produces — not a toast over a still-"Searching…" screen
    // (the device-observed forever-scan). Pins the engine-level outcome so a
    // refactor of the BLE fail path to ShowToast would fail here.
    let mut engine = enter_ble_mode("mode:magic");
    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "ble".into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_failed",
        "a mid-session BLE permission denial must flip the live wait to the failed screen"
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
        landed, "exchange",
        "Cancel should return to the re-armed mode picker (canonical tab-root id)"
    );
    assert!(
        !matches!(engine.current_app_screen(), AppScreen::BleExchange { .. }),
        "Cancel must navigate off the BleExchange AppScreen, still on {:?}",
        engine.current_app_screen()
    );
}

// @internal
#[cfg(feature = "testing")]
#[test]
fn ble_discovery_times_out_via_poll_notifications_past_budget() {
    // The wait-forever fix: a peerless BLE discovery ("Searching…") must fail
    // once the engine's stall budget (BLE_STEP_TIMEOUT_SECS = 60s) elapses,
    // driven by the `poll_notifications` pump (the only non-test tick driver).
    // Reproduces the live Pixel 3a observation (discovery never timed out)
    // through the real AppEngine surface with a FakeClock instead of a wait
    // (CC-06). Pairs with the Android app-level pump that calls
    // pollNotifications on every Ready screen.
    let fake = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let clock: Arc<dyn Clock> = fake.clone();
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:magic".into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering",
        "Magic should land on the live BLE discovering screen"
    );

    // Below the 60s budget: a poll must NOT trip the timeout.
    fake.advance(Duration::from_secs(55));
    let _ = engine.poll_notifications();
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering",
        "must still be discovering before the 60s stall budget"
    );

    // Past the budget (total 65s): a poll MUST fail the stalled discovery.
    fake.advance(Duration::from_secs(10));
    let _ = engine.poll_notifications();
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_failed",
        "poll_notifications past BLE_STEP_TIMEOUT_SECS must fail the peerless BLE discovery"
    );
}
