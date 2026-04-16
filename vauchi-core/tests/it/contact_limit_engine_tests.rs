// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// ── Test helpers ────────────────────────────────────────────────────

fn make_engine() -> ContactLimitEngine {
    ContactLimitEngine::new(5, 50)
}

// ── Tests ───────────────────────────────────────────────────────────

// @internal
#[test]
fn contact_limit_screen_id() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_limit");
}

// @internal
#[test]
fn contact_limit_initial_shows_edit_action() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "edit");
}

// @internal
#[test]
fn contact_limit_edit_shows_save_and_cancel() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.actions.len(), 2);
            assert_eq!(screen.actions[0].id, "save");
            assert_eq!(screen.actions[1].id, "cancel_edit");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_limit_save_valid_number_completes() {
    let mut engine = make_engine();
    // Enter edit mode
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    // Set a valid number
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "100".into(),
    });
    // Save
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn contact_limit_save_invalid_number_returns_validation_error() {
    let mut engine = make_engine();
    // Enter edit mode
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    // Set an invalid value
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "not_a_number".into(),
    });
    // Save
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "limit_input");
            assert_eq!(message, "Please enter a valid number");
        }
        other => panic!("Expected ValidationError, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_limit_cancel_edit_returns_to_view_mode() {
    let mut engine = make_engine();
    // Enter edit mode
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    // Cancel
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_edit".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            // Back to view mode: only "edit" action
            assert_eq!(screen.actions.len(), 1);
            assert_eq!(screen.actions[0].id, "edit");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_limit_text_changed_updates_value() {
    let mut engine = make_engine();
    // Enter edit mode first
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "75".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            // The TextInput should reflect the new value (with cursor indicator)
            match &screen.components[2] {
                Component::TextInput { value, .. } => {
                    assert!(
                        value.contains("75"),
                        "Expected value to contain '75', got: {}",
                        value
                    );
                }
                other => panic!("Expected TextInput, got {:?}", other),
            }
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_limit_usage_percentage_display() {
    let engine = ContactLimitEngine::new(25, 50);
    let screen = engine.current_screen();

    match &screen.components[1] {
        Component::Text { content, .. } => {
            assert!(
                content.contains("50%"),
                "Expected usage to contain '50%', got: {}",
                content
            );
            assert!(
                content.contains("25 / 50"),
                "Expected usage to contain '25 / 50', got: {}",
                content
            );
        }
        other => panic!("Expected Text component for usage, got {:?}", other),
    }
}
