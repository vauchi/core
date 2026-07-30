// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lock screen renders a masked free-text password field, not a
//! fixed-length numeric PinInput.
//!
//! Regression suite for the P0 lockout bug (2026-07-03 GUI audit,
//! `2026-07-03-lock-screen-pin-cap-locks-out-passwords`): the app
//! password is free-text up to 128 chars and the duress PIN is typed
//! into the same field, so the unlock surface must accept the whole
//! credential unchanged. A 6-slot numeric PinInput locked both out.

use vauchi_app::ui::*;

fn lock_input(screen: &ScreenModel) -> (&str, &InputType, &Option<String>) {
    match screen
        .components
        .iter()
        .find(|c| matches!(c, Component::TextInput { id, .. } if id == "pin"))
        .expect("lock screen must render a TextInput with id 'pin'")
    {
        Component::TextInput {
            value,
            input_type,
            validation_error,
            ..
        } => (value.as_str(), input_type, validation_error),
        _ => unreachable!(),
    }
}

fn enter(engine: &mut LockScreenEngine, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: value.into(),
    });
}

fn unlock_enabled(screen: &ScreenModel) -> bool {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "unlock")
        .expect("should have unlock action")
        .enabled
}

// @internal
#[test]
fn lock_screen_id() {
    let engine = LockScreenEngine::new(3);
    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

// @internal
#[test]
fn lock_screen_renders_masked_password_field_not_pin_input() {
    let engine = LockScreenEngine::new(3);
    let screen = engine.current_screen();
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::PinInput { .. })),
        "lock screen must not render a fixed-length PinInput"
    );
    let (_, input_type, _) = lock_input(&screen);
    assert_eq!(
        *input_type,
        InputType::Password,
        "the credential field must be a masked password input"
    );
}

// @internal
#[test]
fn lock_screen_unlock_disabled_when_empty() {
    let engine = LockScreenEngine::new(3);
    assert!(
        !unlock_enabled(&engine.current_screen()),
        "unlock should be disabled when the field is empty"
    );
}

// @internal
#[test]
fn lock_screen_full_value_enables_unlock() {
    let mut engine = LockScreenEngine::new(3);
    enter(&mut engine, "1234");
    assert!(unlock_enabled(&engine.current_screen()));
}

// @internal
#[test]
fn lock_screen_submit_returns_complete() {
    let mut engine = LockScreenEngine::new(3);
    enter(&mut engine, "123456");
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}

// Enter/return in the password field emits `submit_pin` (the `submit_{id}`
// convention, TextInput). It must unlock exactly like the "unlock" button —
// else Enter-to-unlock is dead in the TUI (regression from PinInput →
// TextInput).
// @internal
#[test]
fn lock_screen_submit_pin_action_unlocks() {
    let mut engine = LockScreenEngine::new(3);
    enter(&mut engine, "my-long-password");
    assert_eq!(
        engine.handle_action(UserAction::ActionPressed {
            action_id: "submit_pin".into(),
        }),
        ActionResult::Complete,
        "Enter (submit_pin) must unlock, not just the rendered unlock button"
    );
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
    assert!(!engine.record_failed_attempt(), "1 attempt is not lockout");

    let screen = engine.current_screen();
    let (_, _, validation_error) = lock_input(&screen);
    let error = validation_error
        .as_ref()
        .expect("a failed attempt must surface a remaining-attempts message");
    assert!(error.contains('2'), "should show 2 remaining, got: {error}");
    assert!(
        error.contains("remaining"),
        "should mention 'remaining', got: {error}"
    );
}

// @internal
#[test]
fn lock_screen_max_attempts_lockout() {
    let mut engine = LockScreenEngine::new(3);
    assert!(!engine.record_failed_attempt()); // 1 of 3
    assert!(!engine.record_failed_attempt()); // 2 of 3
    assert!(
        engine.record_failed_attempt(),
        "should lock out at max attempts"
    ); // 3 of 3
}

// The TUI has no local edit buffer — it reconstructs the field from the
// value core echoes, appending each keystroke. If core stops echoing the
// value, TUI unlock silently degrades to last-char-only. Lock that in.
// @internal
#[test]
fn lock_screen_echoes_entered_value_for_tui_accumulation() {
    let mut engine = LockScreenEngine::new(3);
    enter(&mut engine, "1");
    assert_eq!(lock_input(&engine.current_screen()).0, "1");
    enter(&mut engine, "12");
    assert_eq!(lock_input(&engine.current_screen()).0, "12");
}

// @internal
#[test]
fn lock_screen_shorter_value_replaces_on_backspace() {
    let mut engine = LockScreenEngine::new(3);
    enter(&mut engine, "123");
    enter(&mut engine, "12"); // backspace in a full-value field sends the shorter value
    let screen = engine.current_screen();
    assert_eq!(lock_input(&screen).0, "12");
    assert!(unlock_enabled(&screen));
}

// @internal
#[test]
fn lock_screen_clearing_disables_unlock() {
    let mut engine = LockScreenEngine::new(3);
    enter(&mut engine, "123");
    enter(&mut engine, "");
    let screen = engine.current_screen();
    assert_eq!(lock_input(&screen).0, "");
    assert!(
        !unlock_enabled(&screen),
        "clearing the field must disable unlock"
    );
}

// Core regression for the lockout: a long alphanumeric password must be
// retained unchanged — no 6-char cap, no numeric-only restriction.
// @internal
#[test]
fn lock_screen_accepts_long_alphanumeric_password() {
    let mut engine = LockScreenEngine::new(5);
    let password = "Tr0ub4dour&3!longphrase";
    enter(&mut engine, password);
    let screen = engine.current_screen();
    assert_eq!(
        lock_input(&screen).0,
        password,
        "the full credential must be retained, not truncated to 6"
    );
    assert!(unlock_enabled(&screen));
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::Lock {
            pin: password.into()
        }),
        "the full password must reach authenticate() unchanged"
    );
}
