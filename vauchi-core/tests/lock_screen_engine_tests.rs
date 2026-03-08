// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

#[test]
fn lock_screen_id() {
    let engine = LockScreenEngine::new(3);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "lock_screen");
}

#[test]
fn lock_screen_has_pin_input() {
    let engine = LockScreenEngine::new(3);
    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::PinInput { id, .. } if id == "pin"))
        .expect("should have a PinInput component with id 'pin'");
    match pin {
        Component::PinInput { masked, .. } => {
            assert!(*masked, "PinInput should be masked");
        }
        _ => unreachable!(),
    }
}

#[test]
fn lock_screen_unlock_disabled_when_empty() {
    let engine = LockScreenEngine::new(3);
    let screen = engine.current_screen();
    let unlock = screen
        .actions
        .iter()
        .find(|a| a.id == "unlock")
        .expect("should have unlock action");
    assert!(
        !unlock.enabled,
        "unlock should be disabled when pin is empty"
    );
}

#[test]
fn lock_screen_text_input_enables_unlock() {
    let mut engine = LockScreenEngine::new(3);
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "1234".into(),
    });
    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("expected UpdateScreen, got {:?}", other),
    };
    let unlock = screen
        .actions
        .iter()
        .find(|a| a.id == "unlock")
        .expect("should have unlock action");
    assert!(
        unlock.enabled,
        "unlock should be enabled after entering pin"
    );
}

#[test]
fn lock_screen_submit_returns_complete() {
    let mut engine = LockScreenEngine::new(3);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn lock_screen_empty_submit_shows_validation() {
    let mut engine = LockScreenEngine::new(3);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "pin");
            assert_eq!(message, "Please enter your password");
        }
        other => panic!("expected ValidationError, got {:?}", other),
    }
}

#[test]
fn lock_screen_failed_attempt_shows_remaining() {
    let mut engine = LockScreenEngine::new(3);
    let locked_out = engine.record_failed_attempt();
    assert!(!locked_out, "should not be locked out after 1 attempt");

    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::PinInput { id, .. } if id == "pin"))
        .expect("should have PinInput");
    match pin {
        Component::PinInput {
            validation_error, ..
        } => {
            let error = validation_error
                .as_ref()
                .expect("should have validation error after failed attempt");
            assert!(
                error.contains("2"),
                "should show 2 remaining attempts, got: {}",
                error
            );
            assert!(
                error.contains("remaining"),
                "should mention 'remaining', got: {}",
                error
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn lock_screen_max_attempts_lockout() {
    let mut engine = LockScreenEngine::new(3);
    assert!(!engine.record_failed_attempt()); // 1 of 3
    assert!(!engine.record_failed_attempt()); // 2 of 3
    assert!(
        engine.record_failed_attempt(),
        "should return true when max attempts reached"
    ); // 3 of 3
}
