// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for RecoveryEngine — the outgoing social recovery flow.
//!
//! Traces to: features/contact_recovery.feature
//! - @recovery @trust: quorum status
//! - @recovery @vouching: voucher collection
//! - @recovery @proof: proof submission

use vauchi_app::ui::*;

fn make_contact(id: &str, name: &str, initials: &str) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        subtitle: None,
        avatar_initials: initials.into(),
        status: None,
        searchable_fields: vec![],
        actions: vec![],
        a11y: None,
    }
}

// Tests in this file exercise the legacy Status → ShowClaimQr →
// CollectVouchers → Complete flow. The engine now starts at the new
// Intro step (added for the Recover-tab core-driven UI per
// `2026-04-04-core-gui-architecture-alignment` 1B.4), so the
// `quorum_*` helpers jump straight to Status so the existing
// assertions still apply.

fn quorum_not_met() -> RecoveryEngine {
    // 1 contact, threshold 3 => quorum not met
    let mut engine = RecoveryEngine::new(vec![make_contact("c1", "Alice", "AL")], 3);
    engine._jump_to_status_for_testing();
    engine
}

fn quorum_met() -> RecoveryEngine {
    // 3 contacts, threshold 3 => quorum met
    let mut engine = RecoveryEngine::new(
        vec![
            make_contact("c1", "Alice", "AL"),
            make_contact("c2", "Bob", "BO"),
            make_contact("c3", "Carol", "CA"),
        ],
        3,
    );
    engine._jump_to_status_for_testing();
    engine
}

// ── Status screen ────────────────────────────────────────────────

// @internal
#[test]
fn recovery_screen_id() {
    let engine = quorum_not_met();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "recovery_status");
}

// @internal
#[test]
fn recovery_title() {
    let engine = quorum_not_met();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Social Recovery");
}

// @internal
#[test]
fn recovery_quorum_not_met_disables_start() {
    let engine = quorum_not_met();
    let screen = engine.current_screen();

    let start_action = screen
        .actions
        .iter()
        .find(|a| a.id == "start_recovery")
        .expect("start_recovery action should exist");
    assert!(
        !start_action.enabled,
        "start_recovery should be disabled when quorum not met"
    );

    let detail = find_info_detail(&screen, "quorum_info", "Trusted Contacts");
    assert_eq!(detail, "1 of 3");

    let quorum_met_detail = find_info_detail(&screen, "quorum_info", "Quorum Met");
    assert_eq!(quorum_met_detail, "No");
}

// @internal
#[test]
fn recovery_quorum_met_enables_start() {
    let engine = quorum_met();
    let screen = engine.current_screen();

    let start_action = screen
        .actions
        .iter()
        .find(|a| a.id == "start_recovery")
        .expect("start_recovery action should exist");
    assert!(
        start_action.enabled,
        "start_recovery should be enabled when quorum met"
    );

    let detail = find_info_detail(&screen, "quorum_info", "Trusted Contacts");
    assert_eq!(detail, "3 of 3");

    let quorum_met_detail = find_info_detail(&screen, "quorum_info", "Quorum Met");
    assert_eq!(quorum_met_detail, "Yes");
}

// @internal
#[test]
fn status_check_shows_no_active_claims() {
    let mut engine = quorum_not_met();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "check_status".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Recovery Status");
            assert_eq!(message, "No active recovery claims.");
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

// ── State transitions ────────────────────────────────────────────

// @scenario: contact_recovery :: Start recovery shows claim QR
#[test]
fn start_recovery_transitions_to_show_claim_qr() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "recovery_status");
            let has_qr = screen
                .components
                .iter()
                .any(|c| matches!(c, Component::QrCode { mode, .. } if *mode == QrMode::Display));
            assert!(has_qr, "should show QR code with claim data");
            let has_cancel = screen.actions.iter().any(|a| a.id == "cancel");
            assert!(has_cancel, "should have cancel action");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @scenario: contact_recovery :: Cancel recovery returns to status
#[test]
fn cancel_from_claim_qr_returns_to_status() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_start = screen.actions.iter().any(|a| a.id == "start_recovery");
            assert!(
                has_start,
                "should be back at status with start_recovery action"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @scenario: contact_recovery :: Wait for voucher shows scanner
#[test]
fn wait_for_voucher_shows_collection_screen() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wait_for_voucher".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_status = screen
                .components
                .iter()
                .any(|c| matches!(c, Component::StatusIndicator { .. }));
            assert!(has_status, "should show voucher collection status");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @scenario: contact_recovery :: Voucher added updates progress
#[test]
fn add_voucher_updates_progress_count() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "wait_for_voucher".into(),
    });

    engine.add_voucher_for_testing("Alice");
    let screen = engine.current_screen();

    let status_detail = screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator { detail, .. } => detail.clone(),
        _ => None,
    });
    assert_eq!(status_detail.as_deref(), Some("1 of 3 vouchers collected"),);
}

// @scenario: contact_recovery :: Threshold met enables submit
#[test]
fn threshold_met_enables_submit() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "wait_for_voucher".into(),
    });

    engine.add_voucher_for_testing("Alice");
    engine.add_voucher_for_testing("Bob");
    engine.add_voucher_for_testing("Carol");

    let screen = engine.current_screen();
    let submit = screen
        .actions
        .iter()
        .find(|a| a.id == "submit_proof")
        .expect("submit_proof action should exist");
    assert!(submit.enabled, "submit should be enabled at threshold");
}

// @scenario: contact_recovery :: Submit proof transitions to complete
#[test]
fn submit_proof_transitions_to_complete() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "wait_for_voucher".into(),
    });
    engine.add_voucher_for_testing("Alice");
    engine.add_voucher_for_testing("Bob");
    engine.add_voucher_for_testing("Carol");

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_proof".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_success = screen.components.iter().any(|c| {
                matches!(
                    c,
                    Component::StatusIndicator { status, .. }
                        if *status == Status::Success
                )
            });
            assert!(has_success, "should show success status");
            let has_done = screen.actions.iter().any(|a| a.id == "done");
            assert!(has_done, "should have done action");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @scenario: contact_recovery :: Done from complete returns Complete
#[test]
fn done_from_complete_returns_complete() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "wait_for_voucher".into(),
    });
    engine.add_voucher_for_testing("Alice");
    engine.add_voucher_for_testing("Bob");
    engine.add_voucher_for_testing("Carol");
    engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_proof".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "done should return Complete"
    );
}

// @internal
#[test]
fn recovery_unknown_action_returns_update_screen() {
    let mut engine = quorum_not_met();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "recovery_status");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @internal
#[test]
fn submit_before_threshold_not_enabled() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "wait_for_voucher".into(),
    });
    engine.add_voucher_for_testing("Alice");
    // Only 1 of 3

    let screen = engine.current_screen();
    let submit = screen.actions.iter().find(|a| a.id == "submit_proof");
    assert!(submit.is_some(), "submit_proof should exist");
    assert!(
        !submit.unwrap().enabled,
        "submit should be disabled below threshold"
    );
}

// ── Multi-device awareness ───────────────────────────────────────

// @scenario: contact_recovery :: Linked devices show hint
#[test]
fn linked_devices_show_multi_device_hint() {
    let mut engine = quorum_met();
    engine.set_linked_device_count(1);
    let screen = engine.current_screen();

    let has_hint = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { id, .. } if id == "multi_device_hint"
        )
    });
    assert!(
        has_hint,
        "should show multi-device hint when devices linked"
    );
}

// @scenario: contact_recovery :: No linked devices hides hint
#[test]
fn no_linked_devices_hides_hint() {
    let engine = quorum_met();
    let screen = engine.current_screen();

    let has_hint = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { id, .. } if id == "multi_device_hint"
        )
    });
    assert!(
        !has_hint,
        "should not show multi-device hint when no other devices"
    );
}

// ── Completion UX ────────────────────────────────────────────────

// @scenario: contact_recovery :: Complete screen shows what is recovered
#[test]
fn complete_screen_explains_what_is_recovered() {
    let mut engine = quorum_met();
    engine.set_claim_data([0xAB; 32]);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_recovery".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "wait_for_voucher".into(),
    });
    engine.add_voucher_for_testing("Alice");
    engine.add_voucher_for_testing("Bob");
    engine.add_voucher_for_testing("Carol");
    engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_proof".into(),
    });

    let screen = engine.current_screen();

    // Should have info about what is/isn't recovered
    let has_recovery_info = screen.components.iter().any(|c| match c {
        Component::Text { content, .. } => {
            content.contains("contact relationships") || content.contains("NOT recovered")
        }
        _ => false,
    });
    assert!(
        has_recovery_info,
        "complete screen should explain what is/isn't recovered"
    );
}

// --- helpers ---

fn find_info_detail(screen: &ScreenModel, panel_id: &str, item_title: &str) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == panel_id => items
                .iter()
                .find(|item| item.title == item_title)
                .map(|item| item.detail.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("InfoItem '{item_title}' not found in panel '{panel_id}'"))
}
