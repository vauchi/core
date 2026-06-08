// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine onboarding flow tests: identity creation, validation, completion routing.

use crate::common;

use common::app_engine_helpers::{drive_onboarding, drive_onboarding_without_name};
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, FormDialogType, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

use proptest::prelude::*;

// @internal
#[test]
fn app_engine_starts_on_onboarding_without_identity() {
    let vauchi = Vauchi::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.current_app_screen(), &AppScreen::Onboarding);
}

// @internal
#[test]
fn app_engine_shows_onboarding_screen() {
    let vauchi = Vauchi::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "identity_check");
    assert!(!screen.title.is_empty());
}

// @internal
#[test]
fn onboarding_complete_navigates_to_home() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    let result = drive_onboarding(&mut engine);

    // Should navigate to Home after onboarding completes (T-1: verify screen_id)
    let ActionResult::NavigateTo(screen) = result else {
        panic!("expected NavigateTo, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "my_info",
        "onboarding completion should navigate to home"
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

// @internal
#[test]
fn app_engine_starts_on_home_with_identity() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

// @internal
#[test]
fn onboarding_completion_without_name_returns_update_screen_with_error() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    let result = drive_onboarding_without_name(&mut engine);

    // AppEngine resolves ValidationError into UpdateScreen with the error
    // injected into the matching component.
    match &result {
        ActionResult::UpdateScreen(screen) => {
            let has_error = screen.components.iter().any(|c| {
                matches!(
                    c,
                    Component::TextInput {
                        id,
                        validation_error: Some(msg),
                        ..
                    } if id == "display_name" && !msg.is_empty()
                )
            });
            assert!(
                has_error,
                "UpdateScreen should have validation_error on display_name, got {result:?}"
            );
        }
        other => panic!(
            "expected UpdateScreen with validation error, got {:?}",
            other
        ),
    }
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Onboarding,
        "should remain on Onboarding when name is missing"
    );
    assert!(
        !engine.has_identity(),
        "no identity should be created without a name"
    );
}

/// Verify that a whitespace-only name is also rejected.
// @internal
#[test]
fn onboarding_completion_with_empty_name_returns_update_screen_with_error() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Intermediate navigation steps — final validation asserted below
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    // Intermediate step: set a whitespace-only name — validation asserted below
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "   ".into(),
    });
    // Try to continue — onboarding engine should reject it
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // AppEngine resolves ValidationError into UpdateScreen with the error
    // injected into the matching component.
    match &result {
        ActionResult::UpdateScreen(screen) => {
            let has_error = screen.components.iter().any(|c| {
                matches!(
                    c,
                    Component::TextInput {
                        id,
                        validation_error: Some(msg),
                        ..
                    } if id == "display_name" && !msg.is_empty()
                )
            });
            assert!(
                has_error,
                "UpdateScreen should have validation_error on display_name, got {result:?}"
            );
        }
        other => panic!(
            "expected UpdateScreen with validation error, got {:?}",
            other
        ),
    }
    assert!(
        !engine.has_identity(),
        "no identity should be created with whitespace-only name"
    );
}

// @internal
#[test]
fn onboarding_complete_creates_identity_in_vauchi() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    assert!(!engine.has_identity());

    // Intermediate step: drive full onboarding — identity persistence asserted below
    let _ = drive_onboarding(&mut engine);

    assert!(
        engine.has_identity(),
        "identity should be persisted after onboarding"
    );
    assert!(
        engine.available_screens().contains(&AppScreen::MyInfo),
        "should have full nav after identity created"
    );
}

// @internal
#[test]
fn home_screen_no_setup_progress() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "my_info");

    let has_progress = screen.components.iter().any(|c| {
        matches!(c, vauchi_app::ui::Component::StatusIndicator { id, ..
        } if id == "setup_progress")
    });
    assert!(!has_progress, "MyInfo should not show setup progress");
}

/// Reproduce the "identity not initialized" bug:
/// After onboarding creates identity via vauchi.create_identity(),
/// navigating to AddField form and completing it should succeed.
// @internal
#[test]
fn add_field_after_onboarding_identity_creation() {
    // Create Vauchi (no identity) + AppEngine — same as TUI startup
    let vauchi: Vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Simulate TUI onboarding: create identity directly on vauchi
    // (TUI navigation.rs does this, not AppEngine.handle_completion)
    engine.vauchi_mut().create_identity("TestUser").unwrap();

    // Navigate to Home (TUI does this after onboarding)
    engine.navigate_to(AppScreen::MyInfo);

    // Navigate to AddField form (TUI does this on 'a' key)
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Single-page form: select type from flat list
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_add_field");

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "Type selection should update screen, got {result:?}"
    );

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "test@example.com".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    // Should succeed (navigate back), NOT show "identity not initialized"
    match &result {
        ActionResult::NavigateTo(_) => {} // Success
        ActionResult::ShowAlert { message, .. } => {
            panic!("AddField failed with: {message}");
        }
        other => panic!("Unexpected result: {other:?}"),
    }
}

// ── stateful proptest: onboarding random actions (CC-13) ─────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Random sequences of UserActions fired at a fresh AppEngine never
    /// panic and always produce a non-empty screen_id. This satisfies
    /// CC-13 (stateful property tests for state machines).
// @internal
    #[test]
    fn onboarding_random_actions_never_panic(
        actions in prop::collection::vec(
            prop_oneof![
                Just(UserAction::ActionPressed { action_id: "create_new".into() }),
                Just(UserAction::ActionPressed { action_id: "have_identity".into() }),
                Just(UserAction::ActionPressed { action_id: "continue".into() }),
                Just(UserAction::ActionPressed { action_id: "skip".into() }),
                Just(UserAction::ActionPressed { action_id: "back".into() }),
                Just(UserAction::ActionPressed { action_id: "start_app".into() }),
                Just(UserAction::ActionPressed { action_id: "exchange".into() }),
                Just(UserAction::ActionPressed { action_id: "import_contacts".into() }),
                Just(UserAction::ActionPressed { action_id: "read_security".into() }),
                Just(UserAction::ActionPressed { action_id: "read_backup".into() }),
                ".*".prop_map(|s| UserAction::TextChanged {
                    component_id: "display_name".into(),
                    value: s,
                }),
            ],
            0..30
        )
    ) {
        let vauchi = Vauchi::in_memory().unwrap();
        let mut engine = AppEngine::new(vauchi);
        for action in actions {
            // Result intentionally discarded — proptest asserts no-panic + non-empty screen_id
            let _ = engine.handle_action(action);
            let screen = engine.current_screen();
            prop_assert!(!screen.screen_id.is_empty(),
                "screen_id must never be empty");
        }
    }
}
