// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// @internal
#[test]
fn backup_starts_at_choose() {
    let engine = BackupRecoveryEngine::new(None, false);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_choose");
    assert!(screen.progress.is_none());
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "create");
    assert_eq!(screen.actions[0].style, ActionStyle::Primary);
    assert_eq!(screen.actions[1].id, "restore");
    assert_eq!(screen.actions[1].style, ActionStyle::Secondary);
}

// @internal
#[test]
fn backup_create_flow_to_password() {
    let mut engine = BackupRecoveryEngine::new(None, false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "create".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "backup_password");
            let progress = screen.progress.as_ref().expect("should have progress");
            assert_eq!(progress.total_steps, 4);
            assert_eq!(progress.current_step, 1);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn backup_restore_flow_to_password() {
    let mut engine = BackupRecoveryEngine::new(None, false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "restore".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "backup_password");
            let progress = screen.progress.as_ref().expect("should have progress");
            assert_eq!(progress.total_steps, 3);
            assert_eq!(progress.current_step, 1);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn backup_password_validation() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);

    // Continue with empty password should fail
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "password");
            assert_eq!(message, "Password is required");
        }
        other => panic!("Expected ValidationError, got {:?}", other),
    }

    // The continue button should be disabled when password is empty
    let screen = engine.current_screen();
    let continue_action = screen.actions.iter().find(|a| a.id == "continue").unwrap();
    assert!(!continue_action.enabled);
}

// @internal
#[test]
fn backup_confirm_password_mismatch() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);

    // Enter password
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "my-secret".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Enter mismatching confirmation
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "wrong".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "confirm_password");
            assert_eq!(message, "Passwords do not match");
        }
        other => panic!("Expected ValidationError, got {:?}", other),
    }
}

// @internal
#[test]
fn backup_confirm_match_to_processing() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);

    // Enter password
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "my-secret".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Enter matching confirmation
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "my-secret".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "backup_processing");
            assert!(screen.actions.is_empty());
            match &screen.components[0] {
                Component::StatusIndicator { status, .. } => {
                    assert_eq!(*status, Status::InProgress);
                }
                other => panic!("Expected StatusIndicator, got {:?}", other),
            }
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn backup_restore_skips_confirm() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), false);

    // Enter password
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "my-secret".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Should go directly to processing, skipping confirm
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "backup_processing");
            let progress = screen.progress.as_ref().expect("should have progress");
            assert_eq!(progress.total_steps, 3);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn backup_processing_complete() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);

    // Navigate to processing
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "pw".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "pw".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Signal completion
    engine.processing_complete();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_complete");
    match &screen.components[0] {
        Component::StatusIndicator { status, .. } => {
            assert_eq!(*status, Status::Success);
        }
        other => panic!("Expected StatusIndicator, got {:?}", other),
    }
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "done");

    // Done should complete
    let mut engine_done = BackupRecoveryEngine::new(Some(BackupMode::Create), false);
    let _ = engine_done.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "pw".into(),
    });
    let _ = engine_done.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine_done.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "pw".into(),
    });
    let _ = engine_done.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    engine_done.processing_complete();
    let result = engine_done.handle_action(UserAction::ActionPressed {
        action_id: "done".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn backup_processing_failed() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), false);

    // Navigate to processing
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "pw".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Signal failure
    engine.processing_failed();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_failed");
    match &screen.components[0] {
        Component::StatusIndicator { status, .. } => {
            assert_eq!(*status, Status::Failed);
        }
        other => panic!("Expected StatusIndicator, got {:?}", other),
    }
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "retry");
    assert_eq!(screen.actions[1].id, "cancel");

    // Retry should go back to password
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "backup_password");
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn backup_back_navigation() {
    let mut engine = BackupRecoveryEngine::new(None, false);

    // Go to create password
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "backup_password");

    // Back to choose
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "backup_choose");
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }

    // Go to create, enter password, go to confirm, then back to password
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "pw".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "backup_confirm");

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "backup_password");
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn backup_processing_complete_guard_ignores_wrong_step() {
    let mut engine = BackupRecoveryEngine::new(None, false);

    // Calling processing_complete from ChooseMode should be a no-op
    engine.processing_complete();
    assert_eq!(engine.current_screen().screen_id, "backup_choose");

    // Calling processing_failed from ChooseMode should be a no-op
    engine.processing_failed();
    assert_eq!(engine.current_screen().screen_id, "backup_choose");
}

// @internal
#[test]
fn processing_screen_shows_kdf_explanation_for_create() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "pw".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "pw".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = result else {
        panic!("Expected NavigateTo");
    };
    let detail = match &screen.components[0] {
        Component::StatusIndicator { detail, .. } => detail.clone(),
        other => panic!("Expected StatusIndicator, got {other:?}"),
    };
    assert!(
        detail.as_deref().unwrap_or("").contains("encryption key"),
        "Processing screen should explain KDF delay: {detail:?}"
    );
}

// @internal
#[test]
fn processing_screen_shows_kdf_explanation_for_restore() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), false);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "pw".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = result else {
        panic!("Expected NavigateTo");
    };
    let detail = match &screen.components[0] {
        Component::StatusIndicator { detail, .. } => detail.clone(),
        other => panic!("Expected StatusIndicator, got {other:?}"),
    };
    assert!(
        detail.as_deref().unwrap_or("").contains("Decrypting"),
        "Restore processing screen should mention decryption: {detail:?}"
    );
}

// @scenario: backup_format_versioning :: Full backup defaults to full level
// @internal
#[test]
fn backup_defaults_to_full_level() {
    let engine = BackupRecoveryEngine::new(None, false);
    assert_eq!(*engine.level(), BackupLevel::Full);
}

// @scenario: backup_format_versioning :: Backup level toggle switches between full and identity-only
// @internal
#[test]
fn backup_level_toggle_switches_to_identity_only_and_back() {
    let mut engine = BackupRecoveryEngine::new(None, false);
    assert_eq!(*engine.level(), BackupLevel::Full);

    // Toggle to identity-only
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "backup_level".into(),
        item_id: "level_toggle".into(),
    });
    assert_eq!(*engine.level(), BackupLevel::IdentityOnly);
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "backup_choose");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }

    // Toggle back to full
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "backup_level".into(),
        item_id: "level_toggle".into(),
    });
    assert_eq!(*engine.level(), BackupLevel::Full);
}

// @internal
// @internal
#[test]
fn backup_password_getter_returns_entered_password() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);
    assert!(engine.password().is_empty());

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "my-secret-pass".into(),
    });
    assert_eq!(engine.password(), "my-secret-pass");
}

// @internal
// @internal
#[test]
fn backup_mode_getter() {
    let engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), false);
    assert_eq!(*engine.mode(), BackupMode::Restore);

    let engine2 = BackupRecoveryEngine::new(None, false);
    assert_eq!(*engine2.mode(), BackupMode::Create);
}

// Restore needs a keyboard/paste path for the backup blob (the engine
// only had a file-picker path before). See
// `2026-06-02-backup-recovery-engine-restore-gap`.

// @internal
#[test]
fn backup_restore_password_screen_offers_paste_field() {
    let engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), false);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_password");
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::TextInput { id, .. } if id == "backup_data")),
        "restore password screen must offer a backup_data paste field"
    );
}

// @internal
#[test]
fn backup_create_password_screen_has_no_paste_field() {
    let engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);
    let screen = engine.current_screen();
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::TextInput { id, .. } if id == "backup_data")),
        "create flow must not show a paste field"
    );
}

// @internal
#[test]
fn backup_restore_captures_pasted_data_and_requires_it() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), false);

    // Password set but no pasted data → continue must fail on backup_data.
    engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "pw".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(&r, ActionResult::ValidationError { component_id, .. } if component_id == "backup_data"),
        "restore must require backup data, got {r:?}"
    );

    // Paste the blob → captured, and continue now advances.
    engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeef".into(),
    });
    assert_eq!(engine.restore_data(), "deadbeef");
    let r2 = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(r2, ActionResult::NavigateTo(_)),
        "with data + password, continue advances, got {r2:?}"
    );
}

// @internal
#[test]
fn backup_password_value_is_reflected_for_keyboard_frontends() {
    // Keyboard frontends (TUI) render the model value; an always-empty
    // value field would drop typed input. The engine must echo it back.
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), false);
    engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "secret".into(),
    });
    let screen = engine.current_screen();
    let pw = screen.components.iter().find_map(|c| match c {
        Component::TextInput { id, value, .. } if id == "password" => Some(value.clone()),
        _ => None,
    });
    assert_eq!(
        pw.as_deref(),
        Some("secret"),
        "password value must be reflected"
    );
}
