// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `recovery_status.rs` — extracted to keep the
//! engine file under the 1000-line src hard limit. Loaded via
//! `#[path]`.

use super::*;

fn engine() -> RecoveryEngine {
    RecoveryEngine::new(vec![], 3)
}

fn engine_with_quorum() -> RecoveryEngine {
    let contacts = (0..3)
        .map(|i| Item {
            id: format!("c{i}"),
            name: format!("Contact {i}"),
            avatar_initials: format!("C{i}"),
            ..Default::default()
        })
        .collect();
    RecoveryEngine::new(contacts, 3)
}

fn press(engine: &mut RecoveryEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn type_old_key(engine: &mut RecoveryEngine, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "old_public_key".into(),
        value: value.into(),
    });
}

// @internal
#[test]
fn starts_on_intro_screen() {
    let engine = engine();
    let screen = engine.build_screen();
    assert_eq!(screen.title, "Social Recovery");
    // Intro shows the "Lost Your Device?" intro panel.
    assert!(
        screen.components.iter().any(|c| matches!(
            c,
            Component::InfoPanel { id, .. } if id == "intro"
        )),
        "Intro screen must include the intro panel"
    );
}

// @internal
#[test]
fn intro_disables_start_when_quorum_short() {
    let engine = engine();
    let screen = engine.build_screen();
    let start = screen
        .actions
        .iter()
        .find(|a| a.id == "start_recovery_process")
        .unwrap();
    assert!(!start.enabled);
    // Warning banner must be visible.
    assert!(
        screen.components.iter().any(|c| matches!(
            c,
            Component::StatusIndicator { id, .. } if id == "low_trusted_warning"
        )),
        "low_trusted_warning must be shown when quorum is short"
    );
}

// @internal
#[test]
fn intro_enables_start_when_quorum_met() {
    let engine = engine_with_quorum();
    let screen = engine.build_screen();
    let start = screen
        .actions
        .iter()
        .find(|a| a.id == "start_recovery_process")
        .unwrap();
    assert!(start.enabled);
}

// @internal
#[test]
fn start_recovery_process_advances_to_enter_old_key() {
    let mut engine = engine_with_quorum();
    let _ = press(&mut engine, "start_recovery_process");
    let screen = engine.build_screen();
    assert_eq!(screen.title, "Create Recovery Claim");
    assert!(engine.is_at_enter_old_key_step());
}

// @internal
#[test]
fn create_claim_disabled_until_64_hex_chars() {
    let mut engine = engine_with_quorum();
    let _ = press(&mut engine, "start_recovery_process");

    // Empty input → disabled
    let screen = engine.build_screen();
    let create = screen
        .actions
        .iter()
        .find(|a| a.id == "create_claim")
        .unwrap();
    assert!(!create.enabled);

    // Short input → still disabled
    type_old_key(&mut engine, "abcd");
    let screen = engine.build_screen();
    let create = screen
        .actions
        .iter()
        .find(|a| a.id == "create_claim")
        .unwrap();
    assert!(!create.enabled);

    // 64+ chars → enabled
    type_old_key(&mut engine, &"a".repeat(64));
    let screen = engine.build_screen();
    let create = screen
        .actions
        .iter()
        .find(|a| a.id == "create_claim")
        .unwrap();
    assert!(create.enabled);
}

// @internal
#[test]
fn create_claim_returns_complete_so_intercept_can_sign() {
    let mut engine = engine_with_quorum();
    let _ = press(&mut engine, "start_recovery_process");
    type_old_key(&mut engine, &"f".repeat(64));

    let result = press(&mut engine, "create_claim");
    assert!(matches!(result, ActionResult::Complete));
    // Engine stays on EnterOldKey until intercept calls
    // set_generated_claim or set_create_claim_error.
    assert!(engine.is_at_enter_old_key_step());
}

// @internal
#[test]
fn set_generated_claim_advances_to_show_generated_claim() {
    let mut engine = engine_with_quorum();
    engine.set_generated_claim("base64claimpayload");

    let screen = engine.build_screen();
    assert_eq!(screen.title, "Recovery Claim Created");
    let claim_text = screen.components.iter().find_map(|c| match c {
        Component::Text { id, content, .. } if id == "claim_data" => Some(content.clone()),
        _ => None,
    });
    assert_eq!(claim_text.as_deref(), Some("base64claimpayload"));
}

// @internal
#[test]
fn set_create_claim_error_keeps_user_on_enter_old_key_screen() {
    let mut engine = engine_with_quorum();
    let _ = press(&mut engine, "start_recovery_process");
    type_old_key(&mut engine, &"a".repeat(64));
    let _ = press(&mut engine, "create_claim");

    engine.set_create_claim_error("Invalid hex");
    assert!(engine.is_at_enter_old_key_step());

    let screen = engine.build_screen();
    let error = screen.components.iter().find_map(|c| match c {
        Component::TextInput {
            id,
            validation_error,
            ..
        } if id == "old_public_key" => validation_error.clone(),
        _ => None,
    });
    assert_eq!(error.as_deref(), Some("Invalid hex"));
}

// @internal
#[test]
fn editing_old_key_clears_validation_error() {
    let mut engine = engine_with_quorum();
    let _ = press(&mut engine, "start_recovery_process");
    engine.set_create_claim_error("Invalid hex");

    type_old_key(&mut engine, "fixing");
    let screen = engine.build_screen();
    let error = screen.components.iter().find_map(|c| match c {
        Component::TextInput {
            id,
            validation_error,
            ..
        } if id == "old_public_key" => validation_error.clone(),
        _ => None,
    });
    assert_eq!(error, None);
}

// @internal
#[test]
fn cancel_from_enter_old_key_returns_to_intro_and_clears_input() {
    let mut engine = engine_with_quorum();
    let _ = press(&mut engine, "start_recovery_process");
    type_old_key(&mut engine, "in-progress-input");
    let _ = press(&mut engine, "cancel");

    let screen = engine.build_screen();
    assert_eq!(screen.title, "Social Recovery");
    assert_eq!(engine.old_key_input(), "");
}

// @internal
#[test]
fn done_on_show_generated_claim_completes_engine() {
    let mut engine = engine_with_quorum();
    engine.set_generated_claim("payload");
    let result = press(&mut engine, "done");
    assert!(matches!(result, ActionResult::Complete));
}
