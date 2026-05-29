// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end exchange mode tests — full flows through ExchangeEngine.
//!
//! Tests the complete user journey: mode selection → group selection →
//! field preview → mode-specific sub-flow → result.

use vauchi_app::ui::*;
use vauchi_core::exchange::mode::ExchangeMode;

fn config_with_mode_selection() -> ExchangeConfig {
    ExchangeConfig {
        own_name: "Alice".to_string(),
        own_qr_data: "alice-qr-payload".to_string(),
        available_groups: vec![
            ("g1".to_string(), "Family".to_string()),
            ("g2".to_string(), "Friends".to_string()),
        ],
        device_capabilities: vauchi_core::exchange::capability::types::DeviceCapabilities {
            has_camera: true,
            has_internet: true,
            ..Default::default()
        },
        mode: None, // triggers mode selection
        card_snapshot: None,
    }
}

// ================================================================
// Glance: full flow
// ================================================================

// @internal
#[test]
fn glance_full_flow_mode_to_qr_to_result() {
    let mut engine = ExchangeEngine::new(
        config_with_mode_selection(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // Step 1: Mode selection screen
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_mode_selection");

    // Step 2: Select Glance mode
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".to_string(),
        item_id: "mode:glance".to_string(),
    });

    // Step 3: Group selection (groups exist in config)
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_group_selection");

    // Step 4: Continue with groups → Field preview
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_field_preview");

    // Step 5: Start exchange → QR show
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_exchange".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_show_qr");

    // Step 6: Continue to scan
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_scan_qr");

    // Step 7: Simulate scan → Verifying
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "bob-qr-payload".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_verifying");

    // Step 8: Mark success
    engine.mark_success();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_success");

    // Step 9: Done
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".to_string(),
    });
    assert_eq!(result, ActionResult::Complete);
}

// ================================================================
// Broadcast: full flow on the legacy QR sub-flow
// ================================================================
//
// Glance + Hover now both hand off to `MultiStageExchange`
// (Pair 4 + Phase 1.E of `2026-05-11-hover-graduation-plan.md`)
// and so leave the Exchange engine entirely; the Hover handoff is
// pinned by `exchange.rs::tests::hover_mode_routes_through_multi_stage_handoff`.
// Broadcast (one-to-many QR) is the next QR-legacy mode in line for
// graduation — until then it's what this test needs to exercise
// the full legacy ExchangeEngine flow end-to-end.

// @internal
#[test]
fn broadcast_full_flow_mode_to_qr_to_result() {
    let mut engine = ExchangeEngine::new(
        config_with_mode_selection(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // Select Broadcast
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:group".to_string(),
        item_id: "mode:broadcast".to_string(),
    });

    // Skip groups
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(
        screen.screen_id, "exchange_show_qr",
        "Broadcast skip-groups goes straight to QR"
    );

    // Complete the QR flow
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "scanned_data".to_string(),
        value: "peer-data".to_string(),
    });
    engine.mark_success();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".to_string(),
    });
    assert_eq!(result, ActionResult::Complete);
}

// ================================================================
// Link: mode-pick + field-preview hand off to LinkExchangeEngine
//
// The link-mode initiator flow (share-url / waiting / retrieving /
// terminal screens, escrow polling, card decrypt) graduated to the
// pure `LinkExchangeEngine` + engine-owned `LinkInitiatorSession`
// (slice 32l Phase 3b). `ExchangeEngine` no longer enters a Link
// sub-flow — it hands off via `ActionResult::StartLinkExchange`,
// which AppEngine routes to construct the new engine. The full
// share → waiting → retrieving → terminal flow is covered in
// `vauchi-app/tests/reachability/link_exchange.rs` and the
// `link_initiator` unit tests.
// ================================================================

// @internal
#[test]
fn link_full_flow_mode_to_field_preview_hands_off() {
    let mut engine = ExchangeEngine::new(
        config_with_mode_selection(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // Select Link mode
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:remote".to_string(),
        item_id: "mode:link".to_string(),
    });

    // Group selection
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_group_selection");

    // Continue → Field preview
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_field_preview");

    // Start exchange → hand off to LinkExchangeEngine
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_exchange".to_string(),
    });
    assert_eq!(
        result,
        ActionResult::StartLinkExchange,
        "Start exchange in Link mode must hand off to LinkExchangeEngine"
    );
}

// ================================================================
// Cross-mode: failed + retry preserves mode
// ================================================================

// @internal
#[test]
fn failed_retry_preserves_glance_mode() {
    let mut engine = ExchangeEngine::new(
        ExchangeConfig {
            mode: Some(ExchangeMode::Glance),
            ..config_with_mode_selection()
        },
        vauchi_core::clock::SystemClock::shared(),
    );
    engine.mark_failed();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_show_qr");
}

// @internal
#[test]
fn failed_retry_preserves_link_mode() {
    let mut engine = ExchangeEngine::new(
        ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            available_groups: vec![],
            ..config_with_mode_selection()
        },
        vauchi_core::clock::SystemClock::shared(),
    );
    engine.mark_failed();
    // Retry in Link mode hands off to LinkExchangeEngine rather than
    // re-entering a Link sub-flow on this engine.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".to_string(),
    });
    assert_eq!(result, ActionResult::StartLinkExchange);
}

// ================================================================
// Progress tracking
//
// Link-flow progress (share-url → waiting → retrieving → terminal)
// graduated to `LinkExchangeEngine::progress`; its per-screen
// progression is covered by `vauchi-app/tests/reachability/link_exchange.rs`.
// ================================================================
