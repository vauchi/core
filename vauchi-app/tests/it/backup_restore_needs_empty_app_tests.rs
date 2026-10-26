// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A backup restores only into an app with no identity. On a device that
//! has one, Restore explains that and where to erase it, instead of
//! offering a replace that always failed (vauchi/private#819).

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{
    ActionResult, BackupMode, BackupRecoveryEngine, Component, ScreenModel, UserAction,
    WorkflowEngine,
};

const NEEDS_EMPTY_APP: &str = "backup_restore_needs_empty";

fn enter_password(engine: &mut BackupRecoveryEngine) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "password".into(),
        value: "secret123".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_data".into(),
        value: "deadbeefcafe".into(),
    });
}

fn choose_restore(engine: &mut BackupRecoveryEngine) -> ScreenModel {
    match engine.handle_action(UserAction::ListItemSelected {
        component_id: "backup_rows".into(),
        item_id: "restore".into(),
    }) {
        ActionResult::NavigateTo(screen) => screen,
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|component| match component {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn has_input(screen: &ScreenModel) -> bool {
    screen
        .components
        .iter()
        .any(|component| matches!(component, Component::TextInput { .. }))
}

// @internal
#[test]
fn restore_on_a_device_with_an_identity_explains_it_needs_an_empty_app() {
    let mut engine = BackupRecoveryEngine::new(None, true, Locale::ENGLISH);

    let screen = choose_restore(&mut engine);

    assert_eq!(
        (
            screen.screen_id.as_str(),
            screen.title.as_str(),
            texts(&screen),
            has_input(&screen),
        ),
        (
            NEEDS_EMPTY_APP,
            get_string(Locale::ENGLISH, "backup.wizard.restore_needs_empty_title").as_str(),
            vec![get_string(
                Locale::ENGLISH,
                "backup.wizard.restore_needs_empty_body"
            )],
            false,
        )
    );
}

// @internal
#[test]
fn a_restore_started_directly_on_a_device_with_an_identity_explains_too() {
    let engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), true, Locale::ENGLISH);

    assert_eq!(engine.current_screen().screen_id, NEEDS_EMPTY_APP);
}

// @internal
#[test]
fn back_from_the_explanation_returns_to_the_choice() {
    let mut engine = BackupRecoveryEngine::new(None, true, Locale::ENGLISH);
    let _ = choose_restore(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "backup_choose"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn the_explanation_ignores_the_password_steps_actions() {
    let mut engine = BackupRecoveryEngine::new(None, true, Locale::ENGLISH);
    let _ = choose_restore(&mut engine);

    for action_id in ["continue", "choose_file", "confirm_replace"] {
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: action_id.into(),
        });
        assert_eq!(
            engine.current_screen().screen_id,
            NEEDS_EMPTY_APP,
            "{action_id} must not leave the explanation"
        );
    }
}

// @internal
#[test]
fn restore_on_an_empty_app_goes_from_password_straight_to_processing() {
    let mut engine = BackupRecoveryEngine::new(None, false, Locale::ENGLISH);
    assert_eq!(choose_restore(&mut engine).screen_id, "backup_password");
    enter_password(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "backup_processing"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn create_backup_flow_unaffected_by_has_identity() {
    let mut engine = BackupRecoveryEngine::new(Some(BackupMode::Create), true, Locale::ENGLISH);
    enter_password(&mut engine);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "backup_confirm"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}
