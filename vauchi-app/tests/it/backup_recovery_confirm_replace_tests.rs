// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for backup restore InlineConfirm when identity exists (SP-19).

use vauchi_app::ui::{
    ActionResult, BackupMode, BackupRecoveryEngine, Component, UserAction, WorkflowEngine,
};

fn enter_password(engine: &mut BackupRecoveryEngine) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "secret123".into(),
    });
    // Restore now requires a pasted backup blob before `continue`; harmless
    // for Create (the field is ignored there).
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeefcafe".into(),
    });
}

// @internal
#[test]
fn restore_with_identity_shows_confirm_replace() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        true,
        vauchi_app::i18n::Locale::English,
    );
    enter_password(&mut engine);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let screen = match result {
        ActionResult::NavigateTo(s) => s,
        other => panic!("expected NavigateTo, got {other:?}"),
    };
    assert_eq!(screen.screen_id, "backup_confirm_replace");
    let has_confirm = screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { id, destructive, .. }
            if id == "replace" && *destructive)
    });
    assert!(has_confirm, "must show destructive InlineConfirm");
}

// @internal
#[test]
fn restore_without_identity_skips_confirm_replace() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );
    enter_password(&mut engine);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let screen = match result {
        ActionResult::NavigateTo(s) => s,
        other => panic!("expected NavigateTo, got {other:?}"),
    };
    assert_eq!(screen.screen_id, "backup_processing");
}

// @internal
#[test]
fn confirm_replace_proceeds_to_processing() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        true,
        vauchi_app::i18n::Locale::English,
    );
    enter_password(&mut engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_replace".into(),
    });
    let screen = match result {
        ActionResult::NavigateTo(s) => s,
        other => panic!("expected NavigateTo, got {other:?}"),
    };
    assert_eq!(screen.screen_id, "backup_processing");
}

// @internal
#[test]
fn cancel_replace_returns_to_password() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        true,
        vauchi_app::i18n::Locale::English,
    );
    enter_password(&mut engine);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_replace".into(),
    });
    let screen = match result {
        ActionResult::NavigateTo(s) => s,
        other => panic!("expected NavigateTo, got {other:?}"),
    };
    assert_eq!(screen.screen_id, "backup_password");
}

// @internal
#[test]
fn create_backup_flow_unaffected_by_has_identity() {
    // Create flow should be identical regardless of has_identity
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        true,
        vauchi_app::i18n::Locale::English,
    );
    enter_password(&mut engine);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let screen = match result {
        ActionResult::NavigateTo(s) => s,
        other => panic!("expected NavigateTo, got {other:?}"),
    };
    // Create goes to ConfirmPassword, not ConfirmReplace
    assert_eq!(screen.screen_id, "backup_confirm");
}
