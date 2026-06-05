// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level integration tests for the graduated Cable (DirectTransport)
//! exchange engine.
//!
//! Cable graduation slice 4 (R5). Pins the routing seam from slices 2-3: picking
//! Cable on the mode picker emits `ActionResult::StartDirectTransport`, the
//! AppEngine navigates to `AppScreen::DirectTransport`, and the live
//! `DirectTransportEngine` drives the USB ceremony through the uniform AppEngine
//! surface. Mirrors `nfc_exchange_app_engine_tests.rs`.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;

/// A fresh AppEngine with an identity + own card (so the DirectTransport factory
/// can build a live `new_usb` session rather than degrading to Failed).
fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// Enter the exchange flow and pick Cable. With no groups the picker hands off
/// directly to the dedicated DirectTransport screen.
fn enter_cable() -> AppEngine {
    let mut engine = engine_with_identity();
    let entry = engine.navigate_to(AppScreen::Exchange);
    // Canonical tab-root id (see canonical_screen_id_tests).
    assert_eq!(entry.screen_id, "exchange");
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:remote".into(),
        item_id: "mode:cable".into(),
    });
    engine
}

// @internal
#[test]
fn selecting_cable_routes_to_direct_transport_and_renders_waiting() {
    let engine = enter_cable();
    assert!(
        matches!(engine.current_app_screen(), AppScreen::DirectTransport),
        "picking Cable should navigate to DirectTransport, got {:?}",
        engine.current_app_screen()
    );
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_direct_waiting",
        "the live DirectTransportEngine should render the USB-connect waiting screen"
    );
}

// @internal
#[test]
fn forwarding_direct_payload_through_app_engine_reaches_the_engine() {
    let mut engine = enter_cable();
    // A forwarded payload must reach the engine, not be dropped by the AppEngine
    // hardware-event guard (routing.rs gate). Garbage drives the engine to
    // Failed, but the point is that it *reached* the live engine (`is_some`).
    let result = engine.handle_hardware_event(Event::DirectPayloadReceived { data: vec![0u8; 8] });
    assert!(
        result.is_some(),
        "the forwarded DirectPayloadReceived must reach the live engine"
    );
}

// @internal
#[test]
fn cancel_on_direct_transport_lands_on_mode_picker() {
    let mut engine = enter_cable();
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::DirectTransport
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
        !matches!(engine.current_app_screen(), AppScreen::DirectTransport),
        "Cancel must navigate off the DirectTransport AppScreen, still on {:?}",
        engine.current_app_screen()
    );
}
