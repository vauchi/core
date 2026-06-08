// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// @internal
#[test]
fn lock_screen_id() {
    let engine = LockScreenEngine::new(3);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "lock_screen");
}

// @internal
#[test]
fn lock_screen_has_pin_input() {
    let engine = LockScreenEngine::new(3);
    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| {
            matches!(c, Component::PinInput { id, ..
        } if id == "pin")
        })
        .expect("should have a PinInput component with id 'pin'");
    match pin {
        Component::PinInput { masked, .. } => {
            assert!(*masked, "PinInput should be masked");
        }
        _ => unreachable!(),
    }
}

// @internal
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

// @internal
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

// @internal
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

// @internal
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

// @internal
#[test]
fn lock_screen_failed_attempt_shows_remaining() {
    let mut engine = LockScreenEngine::new(3);
    let locked_out = engine.record_failed_attempt();
    assert!(!locked_out, "should not be locked out after 1 attempt");

    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| {
            matches!(c, Component::PinInput { id, ..
        } if id == "pin")
        })
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

// @internal
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

// @internal
#[test]
fn lock_screen_pin_accumulates_single_chars() {
    let mut engine = LockScreenEngine::new(3);

    // Type "1", "2", "3", "4" one char at a time (simulating TUI key presses)
    for ch in ['1', '2', '3', '4'] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }

    // The unlock button should be enabled (pin is non-empty)
    let screen = engine.current_screen();
    let unlock = screen
        .actions
        .iter()
        .find(|a| a.id == "unlock")
        .expect("should have unlock action");
    assert!(
        unlock.enabled,
        "unlock should be enabled after typing 4 chars"
    );

    let pin = screen
        .components
        .iter()
        .find(|c| {
            matches!(c, Component::PinInput { id, ..
        } if id == "pin")
        })
        .expect("should have PinInput");
    match pin {
        Component::PinInput { filled, .. } => {
            assert_eq!(
                *filled, 4,
                "should have 4 filled positions after typing 1234"
            );
        }
        _ => unreachable!(),
    }
}

// @internal
#[test]
fn lock_screen_pin_backspace_removes_last_char() {
    let mut engine = LockScreenEngine::new(3);

    for ch in ['1', '2', '3'] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }

    // Backspace (empty value = delete last char)
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: String::new(),
    });

    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| {
            matches!(c, Component::PinInput { id, ..
        } if id == "pin")
        })
        .expect("should have PinInput");
    match pin {
        Component::PinInput { filled, .. } => {
            assert_eq!(
                *filled, 2,
                "should have 2 filled positions after typing 123 then backspace"
            );
        }
        _ => unreachable!(),
    }
}

// @internal
#[test]
fn lock_screen_pin_backspace_on_empty_is_noop() {
    let mut engine = LockScreenEngine::new(3);

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: String::new(),
    });

    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| {
            matches!(c, Component::PinInput { id, ..
        } if id == "pin")
        })
        .expect("should have PinInput");
    match pin {
        Component::PinInput { filled, .. } => {
            assert_eq!(
                *filled, 0,
                "should have 0 filled positions after backspace on empty"
            );
        }
        _ => unreachable!(),
    }

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

// @internal
#[test]
fn lock_screen_pin_does_not_exceed_length() {
    let mut engine = LockScreenEngine::new(3);

    // Type 8 chars (length is 6)
    for ch in ['1', '2', '3', '4', '5', '6', '7', '8'] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }

    let screen = engine.current_screen();
    let pin = screen
        .components
        .iter()
        .find(|c| {
            matches!(c, Component::PinInput { id, ..
        } if id == "pin")
        })
        .expect("should have PinInput");
    match pin {
        Component::PinInput { filled, length, .. } => {
            assert_eq!(*filled, 6, "filled should be capped at length");
            assert_eq!(*length, 6);
        }
        _ => unreachable!(),
    }
}
