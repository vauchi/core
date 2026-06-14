// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn make_engine() -> ExchangeEngine {
    ExchangeEngine::new(
        ExchangeConfig {
            own_name: "Alice".to_string(),
            own_qr_data: "alice-qr-payload".to_string(),
            available_groups: vec![],
            device_capabilities: Default::default(),
            transport_readiness: Default::default(),
            mode: Some(vauchi_core::exchange::mode::ExchangeMode::Glance),
            card_snapshot: None,
            available_group_data: Vec::new(),
        },
        vauchi_core::clock::SystemClock::shared(),
    )
}

// @internal
#[test]
fn exchange_mark_success() {
    let mut engine = make_engine();
    // Move to scan → verifying
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });

    engine.mark_success();

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_success");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 7);

    match &screen.components[0] {
        Component::StatusIndicator { title, status, .. } => {
            assert_eq!(title, "Exchange Complete");
            assert_eq!(status, &Status::Success);
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
}

// @internal
#[test]
fn exchange_mark_failed() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });

    engine.mark_failed();

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_failed");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 8);

    match &screen.components[0] {
        Component::StatusIndicator { title, status, .. } => {
            assert_eq!(title, "Exchange Failed");
            assert_eq!(status, &Status::Failed);
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }

    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "retry");
    assert_eq!(screen.actions[0].style, ActionStyle::Primary);
    assert_eq!(screen.actions[1].id, "cancel");
    assert_eq!(screen.actions[1].style, ActionStyle::Secondary);
}

// @internal
#[test]
fn exchange_success_done_completes() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    engine.mark_success();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".to_string(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn exchange_failed_retry_restarts() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    engine.mark_failed();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".to_string(),
    });

    // Glance retry hands back to the multi-stage engine (mode preserved),
    // not the legacy QR step — Retry now routes through the shared
    // `enter_mode_sub_flow` router like the forward paths.
    match result {
        ActionResult::StartBleExchange { mode } => {
            assert_eq!(mode, vauchi_core::exchange::mode::ExchangeMode::Glance);
        }
        other => panic!("expected StartBleExchange(Glance) [G3], got {:?}", other),
    }

    // Scanned data should be cleared on retry. (The StartMultiStageExchange
    // result above is the meaningful "restart" — Glance hands off to the
    // multi-stage engine; the cached ExchangeEngine's own screen is left
    // behind, so it's not asserted here.)
    assert_eq!(engine.scanned_data(), None);
}

// @internal
#[test]
fn exchange_failed_cancel_completes() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    engine.mark_failed();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".to_string(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @scenario: accessibility :: Exchange success screen StatusIndicator has populated a11y
//
// Verifies that the success StatusIndicator carries a meaningful accessibility
// label so screen readers can announce the outcome to users.
// @internal
#[test]
fn exchange_success_status_indicator_has_a11y() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    engine.mark_success();

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_success");

    match &screen.components[0] {
        Component::StatusIndicator { a11y, .. } => {
            let a11y = a11y
                .as_ref()
                .expect("success StatusIndicator must have a11y populated");
            assert_eq!(
                a11y.label.as_deref(),
                Some("Exchange complete"),
                "a11y label should describe the outcome"
            );
            assert!(
                a11y.hint.is_some(),
                "a11y hint should be present to explain what happened"
            );
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
}

// ===== Debug log auto-enabled =====

// @internal
#[test]
fn with_session_auto_enables_debug_log() {
    let identity = vauchi_core::identity::Identity::create("Alice", 0);
    let card = vauchi_core::contact_card::ContactCard::new("Alice");
    let proximity = vauchi_core::exchange::MockProximityVerifier::success();
    let session = vauchi_core::exchange::ExchangeSession::new_qr(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    let config = ExchangeConfig {
        own_name: "Alice".to_string(),
        own_qr_data: "alice-payload".to_string(),
        available_groups: vec![],
        device_capabilities: Default::default(),
        transport_readiness: Default::default(),
        mode: Some(vauchi_core::exchange::mode::ExchangeMode::Glance),
        card_snapshot: None,
        available_group_data: Vec::new(),
    };

    let engine =
        ExchangeEngine::with_session(config, session, vauchi_core::clock::SystemClock::shared());

    let log = engine
        .session()
        .unwrap()
        .exchange_debug_log()
        .expect("debug log should be auto-enabled by with_session");
    assert!(
        !log.events().is_empty(),
        "auto-enabled log should have events"
    );
    assert!(matches!(
        &log.events()[0].event,
        vauchi_core::diagnostic::exchange_debug::ExchangeDebugEvent::SessionStarted { .. }
    ));
}

// @scenario: accessibility :: Exchange fallback actions carry consequence hints
//
// Selective a11y (2026-05-29 decision, record
// `2026-05-29-screenaction-a11y-convention`): a ScreenAction whose label
// names a transport/mode with a non-obvious consequence carries an a11y
// hint (label/role left None — the visible label is the accessible name).
// The Failed screen's fallback_qr / fallback_relay get hints; the
// plain-verb retry / cancel stay None.
// @internal
#[test]
fn exchange_failed_fallback_actions_have_a11y_hints() {
    // Reach Failed with BOTH fallbacks: a camera-capable device that
    // BLE-disconnects mid-flow sets ble_fallback_available and (camera →)
    // qr_fallback_available.
    // Post BLE graduation the BLE flow lives in the dedicated
    // `BleExchangeEngine`; `has_camera = true` gates the QR fallback offer.
    let mut engine = BleExchangeEngine::new(
        vauchi_core::exchange::mode::ExchangeMode::Magic,
        true,
        vec![],
        vauchi_core::clock::SystemClock::shared(),
    );
    let _ = engine.handle_hardware_event(vauchi_core::Event::BleDisconnected {
        reason: "peer hung up".to_string(),
    });

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_failed");

    let find = |id: &str| {
        screen
            .actions
            .iter()
            .find(|a| a.id == id)
            .unwrap_or_else(|| panic!("action {id} must be present on the Failed screen"))
    };

    // fallback_qr — hint names the consequence; label/role stay None.
    let qr = find("fallback_qr")
        .a11y
        .as_ref()
        .expect("fallback_qr must carry a11y");
    assert_eq!(
        qr.label, None,
        "fallback_qr a11y must not duplicate the visible label"
    );
    assert_eq!(
        qr.role, None,
        "fallback_qr a11y must not set a redundant role"
    );
    assert_eq!(
        qr.hint.as_deref(),
        Some("Abandons this attempt and restarts the exchange using camera QR codes."),
    );

    // fallback_relay — hint names the consequence; label/role stay None.
    let relay = find("fallback_relay")
        .a11y
        .as_ref()
        .expect("fallback_relay must carry a11y");
    assert_eq!(relay.label, None);
    assert_eq!(relay.role, None);
    assert_eq!(
        relay.hint.as_deref(),
        Some("Abandons this attempt and completes the exchange over the encrypted relay server."),
    );

    // Plain-verb actions stay None — the visible label is the accessible name.
    assert_eq!(find("retry").a11y, None, "retry is self-evident");
    assert_eq!(find("cancel").a11y, None, "cancel is self-evident");
}
