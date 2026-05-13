// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end exchange mode tests — full flows through ExchangeEngine.
//!
//! Tests the complete user journey: mode selection → group selection →
//! field preview → mode-specific sub-flow → result.

use vauchi_app::ui::*;
use vauchi_core::Command;
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
// Link: full flow
// ================================================================

// @internal
#[test]
fn link_full_flow_mode_to_share_to_waiting() {
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

    // Start exchange → Link share URL (emits presence deposit command)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_exchange".to_string(),
    });
    assert!(
        matches!(result, ActionResult::Commands { ref commands } if commands.iter().any(|c| matches!(c, Command::RelayEscrowDeposit { .. }))),
        "Start exchange in Link mode must emit RelayEscrowDeposit (presence)"
    );
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_share_url");

    // Share → Waiting (emits ShowShareSheet command)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "share".to_string(),
    });
    assert!(
        matches!(result, ActionResult::Commands { ref commands } if commands.iter().any(|c| matches!(c, Command::ShowShareSheet { .. }))),
        "Share must emit ShowShareSheet command"
    );
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_link_waiting");

    // Cancel from waiting
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".to_string(),
    });
    assert_eq!(result, ActionResult::Complete);
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
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_share_url");
}

// ================================================================
// Progress tracking
// ================================================================

// @internal
#[test]
fn progress_advances_through_link_flow() {
    let mut engine = ExchangeEngine::new(
        ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            available_groups: vec![],
            ..config_with_mode_selection()
        },
        vauchi_core::clock::SystemClock::shared(),
    );

    // Link starts at step 4 (after mode=1, groups=2, preview=3)
    let s1 = engine.current_screen();
    let step1 = s1.progress.as_ref().unwrap().current_step;

    // Share → Waiting (step should advance)
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "share".to_string(),
    });
    let s2 = engine.current_screen();
    let step2 = s2.progress.as_ref().unwrap().current_step;

    assert!(
        step2 > step1,
        "Step must advance from ShareUrl ({step1}) to WaitingForResponse ({step2})"
    );
    assert_eq!(s2.progress.as_ref().unwrap().total_steps, 8);
}
