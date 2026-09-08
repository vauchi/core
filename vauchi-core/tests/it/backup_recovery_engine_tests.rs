// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// @internal
#[test]
fn backup_starts_at_choose() {
    let engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_choose");
    assert!(screen.progress.is_none());
    assert_eq!(screen.contextual_actions.len(), 2);
    assert_eq!(screen.contextual_actions[0].id, "create");
    assert_eq!(screen.contextual_actions[0].style, ActionStyle::Primary);
    assert_eq!(screen.contextual_actions[1].id, "restore");
    assert_eq!(screen.contextual_actions[1].style, ActionStyle::Secondary);
}

// @internal
#[test]
fn backup_create_flow_to_password() {
    let mut engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);
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
    let mut engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);
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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "password");
            // Converged on the canonical backup.error_enter_password key
            // value (M3 S3d) — edits go through the locale files now.
            assert_eq!(message, "Please enter your backup password");
        }
        other => panic!("Expected ValidationError, got {:?}", other),
    }

    let screen = engine.current_screen();
    let continue_action = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "continue")
        .unwrap();
    assert!(!continue_action.enabled);
}

// @internal
#[test]
fn backup_confirm_password_mismatch() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );

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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );

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
            assert!(screen.contextual_actions.is_empty());
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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );

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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );

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

    engine.processing_complete();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_complete");
    match &screen.components[0] {
        Component::StatusIndicator { status, .. } => {
            assert_eq!(*status, Status::Success);
        }
        other => panic!("Expected StatusIndicator, got {:?}", other),
    }
    assert_eq!(screen.contextual_actions.len(), 1);
    assert_eq!(screen.contextual_actions[0].id, "done");

    let mut engine_done = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );
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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );

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

    engine.processing_failed();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_failed");
    match &screen.components[0] {
        Component::StatusIndicator { status, .. } => {
            assert_eq!(*status, Status::Failed);
        }
        other => panic!("Expected StatusIndicator, got {:?}", other),
    }
    assert_eq!(screen.contextual_actions.len(), 2);
    assert_eq!(screen.contextual_actions[0].id, "retry");
    assert_eq!(screen.contextual_actions[1].id, "cancel");

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
    let mut engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "backup_password");

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
    let mut engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);

    engine.processing_complete();
    assert_eq!(engine.current_screen().screen_id, "backup_choose");

    engine.processing_failed();
    assert_eq!(engine.current_screen().screen_id, "backup_choose");
}

// @internal
#[test]
fn processing_screen_shows_kdf_explanation_for_create() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );
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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );
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
    let engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);
    assert_eq!(*engine.level(), BackupLevel::Full);
}

// @scenario: backup_format_versioning :: Backup level toggle switches between full and identity-only
// @internal
#[test]
fn backup_level_toggle_switches_to_identity_only_and_back() {
    let mut engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);
    assert_eq!(*engine.level(), BackupLevel::Full);

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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );
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
    let engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );
    assert_eq!(*engine.mode(), BackupMode::Restore);

    let engine2 = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);
    assert_eq!(*engine2.mode(), BackupMode::Create);
}

// Restore needs a keyboard/paste path for the backup blob (the engine
// only had a file-picker path before). See
// `2026-06-02-backup-recovery-engine-restore-gap`.

// @internal
#[test]
fn backup_restore_password_screen_offers_paste_field() {
    let engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );
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
    let engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );
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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );

    // Password set but no pasted data → continue must fail on backup_data.
    let _ = engine.handle_action(UserAction::TextChanged {
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
    let _ = engine.handle_action(UserAction::TextChanged {
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
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );
    let _ = engine.handle_action(UserAction::TextChanged {
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

// @feature: silent_failure_modes
// @scenario: backup_recovery :: a second submit during processing cannot re-enter the flow
/// Mashing the submit button during a long restore must not advance,
/// restart, or consume anything.
///
/// This is the second half of the test strategy in problem record
/// 2026-06-11-restore-runs-without-progress-feedback. The device symptom
/// was a ~90 s restore behind an unchanged, still-interactive screen: the
/// user assumed the tap missed and pressed again, and because the engine
/// takes the staged backup bytes on first submit, the second press
/// silently did nothing. Rejection has to come from the state machine —
/// a debounce would still leave the flow re-enterable.
// @internal
#[test]
fn backup_processing_rejects_a_second_submit() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );

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
    assert_eq!(
        engine.current_screen().screen_id,
        "backup_processing",
        "restore submit must land on the processing screen"
    );

    for action_id in ["continue", "restore", "back"] {
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: action_id.into(),
        });
        match result {
            ActionResult::UpdateScreen(screen) => assert_eq!(
                screen.screen_id, "backup_processing",
                "'{action_id}' during processing must not leave the screen"
            ),
            other => panic!("'{action_id}' during processing must be a no-op, got {other:?}"),
        }
    }

    assert_eq!(
        engine.current_screen().screen_id,
        "backup_processing",
        "the engine must still be processing after repeated submits"
    );
    assert!(
        engine.current_screen().contextual_actions.is_empty(),
        "processing must expose no affordances to press in the first place"
    );

    engine.processing_complete();
    assert_eq!(
        engine.current_screen().screen_id,
        "backup_complete",
        "completion must remain the only exit"
    );
}

// @feature: backup_recovery
/// Restore must offer a file path, because a real backup cannot be pasted.
///
/// A 10k-contact export is 2,278,510 bytes and Core bounds a presentation
/// input value at 4,096 (`MAX_EVENT_INPUT_VALUE_BYTES`), so the paste field
/// silently never receives it — the field stays empty and Continue stays
/// disabled with nothing explaining why (problem record
/// 2026-09-08-full-backup-restore-cannot-be-pasted). File bytes arrive as
/// `Event::FilePickedFromUser` and are not subject to that bound.
// @scenario: backup_recovery :: restore offers a file pick for full backups
#[test]
fn restore_password_screen_offers_a_file_pick_action() {
    let engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );
    let screen = engine.current_screen();
    assert!(
        screen
            .contextual_actions
            .iter()
            .any(|a| a.id == "choose_file"),
        "restore must offer a file pick, got: {:?}",
        screen
            .contextual_actions
            .iter()
            .map(|a| &a.id)
            .collect::<Vec<_>>()
    );
}

// @feature: backup_recovery
/// Creating a backup writes a file out; it never reads one in, so the
/// pick affordance must not appear there.
// @scenario: backup_recovery :: create mode has no file pick
#[test]
fn create_password_screen_has_no_file_pick_action() {
    let engine = BackupRecoveryEngine::new(
        Some(BackupMode::Create),
        false,
        vauchi_app::i18n::Locale::English,
    );
    assert!(
        !engine
            .current_screen()
            .contextual_actions
            .iter()
            .any(|a| a.id == "choose_file"),
        "create mode must not offer a file pick"
    );
}

// @feature: backup_recovery
/// The pick affordance asks the shell for a file, using the purpose the
/// device-replacement flow already uses rather than a new one.
// @scenario: backup_recovery :: choosing a file requests an ImportBackup pick
#[test]
fn choose_file_requests_an_import_backup_pick() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "choose_file".into(),
    });
    match result {
        ActionResult::Commands { commands } => match &commands[0] {
            vauchi_core::Command::FilePickFromUser { purpose, .. } => {
                assert_eq!(*purpose, vauchi_core::FilePickPurpose::ImportBackup);
            }
            other => panic!("expected FilePickFromUser, got {other:?}"),
        },
        other => panic!("expected Commands, got {other:?}"),
    }
}

// @feature: backup_recovery
/// Picked bytes populate the restore payload without passing through the
/// bounded text-input channel, and the flow becomes submittable once the
/// password is entered.
// @scenario: backup_recovery :: picked backup bytes populate the restore payload
#[test]
fn picked_backup_bytes_populate_the_restore_payload() {
    let mut engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        false,
        vauchi_app::i18n::Locale::English,
    );

    // Far past MAX_EVENT_INPUT_VALUE_BYTES: the point is that this path
    // has no such ceiling.
    let payload = "a".repeat(64 * 1024);
    assert!(engine.apply_update(EngineUpdate::BackupRecovery(
        BackupRecoveryUpdate::PickedBackupBytes(payload.clone().into_bytes()),
    )));

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "my-secret".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "backup_password");
    assert!(
        screen
            .contextual_actions
            .iter()
            .any(|a| a.id == "continue" && a.enabled),
        "continue must enable once a file and password are present"
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    match result {
        ActionResult::NavigateTo(next) => assert_eq!(
            next.screen_id, "backup_processing",
            "submitting a picked backup must show progress, not sit on an unchanged screen"
        ),
        other => panic!("expected NavigateTo(backup_processing), got {other:?}"),
    }
}
