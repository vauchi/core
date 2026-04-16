// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that `AppEngine` converts `ActionResult::ValidationError` into
//! `ActionResult::UpdateScreen` with the error injected into the matching
//! component. Frontends should never receive `ValidationError` directly.

use vauchi_app::ui::{ActionResult, AppEngine, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

// @scenario: onboarding.feature :: Empty name rejected with inline validation error
/// Pressing "continue" on the onboarding name screen with an empty name
/// must return `UpdateScreen` (not `ValidationError`) with the error
/// injected into the `display_name` TextInput component.
// @internal
#[test]
fn onboarding_empty_name_returns_update_screen_with_error() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // We start on onboarding. Navigate to the name step.
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });

    // Now on DefaultName step. Submit with empty name.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match &result {
        ActionResult::UpdateScreen(screen) => {
            let text_input = screen
                .components
                .iter()
                .find(|c| matches!(c, Component::TextInput { id, .. } if id == "display_name"));
            assert!(
                text_input.is_some(),
                "Screen must contain the display_name TextInput"
            );
            if let Some(Component::TextInput {
                validation_error, ..
            }) = text_input
            {
                assert!(
                    validation_error.is_some(),
                    "validation_error must be set on the TextInput"
                );
                let error_msg = validation_error.as_ref().unwrap();
                assert!(
                    error_msg.contains("name"),
                    "Error message should mention name, got: {error_msg}"
                );
            }
        }
        other => panic!(
            "Expected UpdateScreen with validation error, got: {:?}",
            other
        ),
    }
}

// @scenario: onboarding.feature :: Valid name after validation error navigates forward
/// After a validation error, submitting with valid input must succeed
/// (navigate away from the name step).
// @internal
#[test]
fn onboarding_valid_name_after_error_navigates_forward() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to name step
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });

    // Trigger validation error
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Now enter a valid name and submit
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "Valid name should navigate forward, got: {:?}",
        result
    );
}

// @scenario: screen_model :: with_validation_error injects error into TextInput
/// The `with_validation_error` method on `ScreenModel` correctly injects
/// errors into `TextInput` components and leaves non-matching components
/// unchanged.
// @internal
#[test]
fn screen_model_with_validation_error_injects_into_text_input() {
    use vauchi_app::ui::{InputType, ScreenModel};

    let screen = ScreenModel {
        screen_id: "test".into(),
        title: "Test".into(),
        components: vec![
            Component::TextInput {
                id: "field_a".into(),
                label: "A".into(),
                value: "".into(),
                placeholder: None,
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
                a11y: None,
            },
            Component::TextInput {
                id: "field_b".into(),
                label: "B".into(),
                value: "".into(),
                placeholder: None,
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
                a11y: None,
            },
        ],
        ..Default::default()
    };

    let screen = screen.with_validation_error("field_a", "Required".into());

    // field_a should have the error
    if let Component::TextInput {
        id,
        validation_error,
        ..
    } = &screen.components[0]
    {
        assert_eq!(id, "field_a");
        assert_eq!(validation_error.as_deref(), Some("Required"));
    } else {
        panic!("Expected TextInput");
    }

    // field_b should be unchanged
    if let Component::TextInput {
        id,
        validation_error,
        ..
    } = &screen.components[1]
    {
        assert_eq!(id, "field_b");
        assert_eq!(*validation_error, None);
    } else {
        panic!("Expected TextInput");
    }
}

// @scenario: screen_model :: with_validation_error injects error into PinInput
/// `with_validation_error` works for `PinInput` components too.
// @internal
#[test]
fn screen_model_with_validation_error_injects_into_pin_input() {
    use vauchi_app::ui::ScreenModel;

    let screen = ScreenModel {
        screen_id: "test".into(),
        title: "Test".into(),
        components: vec![Component::PinInput {
            id: "pin".into(),
            label: "Enter PIN".into(),
            length: 6,
            filled: 0,
            masked: true,
            validation_error: None,
            a11y: None,
        }],
        ..Default::default()
    };

    let screen = screen.with_validation_error("pin", "Wrong PIN".into());

    if let Component::PinInput {
        validation_error, ..
    } = &screen.components[0]
    {
        assert_eq!(validation_error.as_deref(), Some("Wrong PIN"));
    } else {
        panic!("Expected PinInput");
    }
}
