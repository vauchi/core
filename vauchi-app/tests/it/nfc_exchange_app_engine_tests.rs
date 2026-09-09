// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level integration tests for the graduated NFC exchange engine.
//!
//! NFC graduation slice 3. Pins the routing seam added in slices 1-2: picking
//! TapTap on the mode picker emits `ActionResult::StartNfcExchange`, the
//! AppEngine navigates to `AppScreen::NfcExchange`, and the live
//! `NfcExchangeEngine` drives the role chooser → tap handshake through the
//! uniform AppEngine surface. Mirrors `ble_exchange_app_engine_tests.rs`.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::nfc_apdu;
use vauchi_core::{Command, Event};

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// Enter the exchange flow and pick TapTap. With no groups the picker hands off
/// directly to the dedicated NFC screen.
fn enter_taptap() -> AppEngine {
    let mut engine = engine_with_identity();
    let entry = engine.navigate_to(AppScreen::Exchange);
    // Canonical tab-root id (see canonical_screen_id_tests).
    assert_eq!(entry.screen_id, "exchange");
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:fun".into(),
        item_id: "mode:tap_tap".into(),
    });
    engine
}

// @internal
#[test]
fn selecting_taptap_routes_to_nfc_exchange_and_renders_role_chooser() {
    let engine = enter_taptap();
    assert!(
        matches!(engine.current_app_screen(), AppScreen::NfcExchange),
        "picking TapTap should navigate to NfcExchange, got {:?}",
        engine.current_app_screen()
    );
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_nfc_role",
        "the live NfcExchangeEngine should render the Send/Receive chooser"
    );
}

// @internal
#[test]
fn picking_send_through_app_engine_emits_nfc_activate() {
    let mut engine = enter_taptap();
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "nfc_role".into(),
        item_id: "nfc_role:send".into(),
    });
    match result {
        ActionResult::Commands { commands } => assert!(
            matches!(&commands[0], vauchi_core::Command::NfcActivate { payload, .. } if !payload.is_empty()),
            "Send should emit NfcActivate with a key-offer payload, got {commands:?}"
        ),
        other => panic!("expected Commands from the active NFC engine, got {other:?}"),
    }
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_nfc_awaiting_tap",
        "after Send the engine awaits the tap"
    );
}

// @internal
#[test]
fn picking_send_emits_nfc_activate_with_framed_apdus() {
    let mut engine = enter_taptap();
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "nfc_role".into(),
        item_id: "nfc_role:send".into(),
    });
    let ActionResult::Commands { commands } = result else {
        panic!("expected Commands from the active NFC engine, got {result:?}");
    };
    let Command::NfcActivate { payload, apdus } = &commands[0] else {
        panic!("expected NfcActivate, got {commands:?}");
    };
    let mut exchange = vec![0x00, 0xE0, 0x00, 0x00, payload.len() as u8];
    exchange.extend_from_slice(payload);
    assert_eq!(apdus, &vec![nfc_apdu::build_select(), exchange]);
}

// @internal
#[test]
fn forwarding_raw_select_apdu_to_the_receiver_is_answered_with_success() {
    let mut engine = enter_taptap();
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "nfc_role".into(),
        item_id: "nfc_role:receive".into(),
    });

    let result = engine.handle_hardware_event(Event::NfcApduReceived {
        bytes: nfc_apdu::build_select(),
    });

    assert_eq!(
        result,
        Some(ActionResult::Commands {
            commands: vec![Command::NfcSendApdu {
                data: vec![0x90, 0x00],
                apdus: vec![vec![0x90, 0x00]],
            }],
        })
    );
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_nfc_awaiting_tap",
        "a SELECT establishes the tap but carries no handshake data"
    );
}

// @internal
#[test]
fn nfc_failed_event_lands_on_the_failed_screen() {
    let mut engine = enter_taptap();
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "nfc_role".into(),
        item_id: "nfc_role:send".into(),
    });

    let result = engine.handle_hardware_event(Event::NfcFailed {
        reason: "tag lost".into(),
    });

    assert!(result.is_some(), "NfcFailed must reach the live engine");
    assert_eq!(engine.current_screen().screen_id, "exchange_failed");
}

// @internal
#[test]
fn forwarding_nfc_data_through_app_engine_reaches_the_engine() {
    let mut engine = enter_taptap();
    // Receive: defers the responder flow to the lazy bootstrap.
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "nfc_role".into(),
        item_id: "nfc_role:receive".into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_nfc_awaiting_tap"
    );

    // A forwarded tap must reach the engine (bootstrapping the responder),
    // not be dropped by the AppEngine hardware-event guard.
    let result = engine.handle_hardware_event(Event::NfcDataReceived { data: vec![0u8; 8] });
    assert!(
        result.is_some(),
        "the forwarded NfcDataReceived must reach the live engine"
    );
}

// @internal
#[test]
fn cancel_on_nfc_role_lands_on_mode_picker() {
    let mut engine = enter_taptap();
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::NfcExchange
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
        !matches!(engine.current_app_screen(), AppScreen::NfcExchange),
        "Cancel must navigate off the NfcExchange AppScreen, still on {:?}",
        engine.current_app_screen()
    );
}
