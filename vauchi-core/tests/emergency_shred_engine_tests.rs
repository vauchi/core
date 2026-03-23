// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

#[test]
fn shred_starts_at_warning() {
    let engine = EmergencyShredEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "shred_warning");
    assert_eq!(screen.progress.as_ref().unwrap().current_step, 1);
    assert_eq!(screen.progress.as_ref().unwrap().total_steps, 3);
}

#[test]
fn shred_warning_has_info_panel() {
    let engine = EmergencyShredEngine::new();
    let screen = engine.current_screen();

    let info_panel = screen.components.first().expect("should have a component");
    match info_panel {
        Component::InfoPanel {
            icon, title, items, ..
        } => {
            assert_eq!(icon.as_deref(), Some("warning"));
            assert_eq!(title, "Emergency Data Wipe");
            assert_eq!(items.len(), 3);
        }
        other => panic!("expected InfoPanel, got {:?}", other),
    }

    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, "continue");
    assert_eq!(screen.actions[0].style, ActionStyle::Destructive);
    assert_eq!(screen.actions[0].label, "I Understand");
    assert_eq!(screen.actions[1].id, "cancel");
    assert_eq!(screen.actions[1].style, ActionStyle::Secondary);
}

#[test]
fn shred_continue_to_confirm() {
    let mut engine = EmergencyShredEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "shred_confirm");
            assert_eq!(screen.progress.as_ref().unwrap().current_step, 2);
        }
        other => panic!("expected NavigateTo, got {:?}", other),
    }
}

// @scenario: emergency_shred :: Hard shred requires valid shred token
#[test]
fn shred_confirm_requires_delete_text() {
    let mut engine = EmergencyShredEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    let wipe_action = screen
        .actions
        .iter()
        .find(|a| a.id == "wipe")
        .expect("should have wipe action");
    assert!(
        !wipe_action.enabled,
        "wipe should be disabled without DELETE text"
    );

    // Type DELETE and check it becomes enabled
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });
    let screen = engine.current_screen();
    let wipe_action = screen
        .actions
        .iter()
        .find(|a| a.id == "wipe")
        .expect("should have wipe action");
    assert!(
        wipe_action.enabled,
        "wipe should be enabled with DELETE text"
    );
}

#[test]
fn shred_confirm_wrong_text_validation_error() {
    let mut engine = EmergencyShredEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "WRONG".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "confirmation");
            assert_eq!(message, "Type DELETE to confirm");
        }
        other => panic!("expected ValidationError, got {:?}", other),
    }
}

// @scenario: emergency_shred :: Panic shred destroys everything immediately
#[test]
fn shred_confirm_delete_starts_wipe() {
    let mut engine = EmergencyShredEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "shred_wiping");
            assert_eq!(screen.progress.as_ref().unwrap().current_step, 3);

            match &screen.components[0] {
                Component::StatusIndicator { status, title, .. } => {
                    assert_eq!(status, &Status::InProgress);
                    assert_eq!(title, "Wiping data...");
                }
                other => panic!("expected StatusIndicator, got {:?}", other),
            }

            assert!(
                screen.actions.is_empty(),
                "wiping screen should have no actions"
            );
        }
        other => panic!("expected NavigateTo, got {:?}", other),
    }
}

// @scenario: emergency_shred :: Shred report tracks what was destroyed
#[test]
fn shred_wipe_complete() {
    let mut engine = EmergencyShredEngine::new();

    // Navigate to Wiping step
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "DELETE".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "wipe".into(),
    });

    // Signal wipe complete
    engine.wipe_complete();

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "shred_complete");
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "done");

    match &screen.components[0] {
        Component::StatusIndicator { status, title, .. } => {
            assert_eq!(status, &Status::Success);
            assert_eq!(title, "Data Wiped");
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }

    // Pressing done returns WipeComplete
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".into(),
    });
    assert_eq!(result, ActionResult::WipeComplete);
}

// @scenario: emergency_shred :: Cancel soft shred during grace period
#[test]
fn shred_cancel_returns_complete() {
    // Cancel from warning
    let mut engine = EmergencyShredEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert_eq!(result, ActionResult::Complete);

    // Cancel from confirm
    let mut engine = EmergencyShredEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}
