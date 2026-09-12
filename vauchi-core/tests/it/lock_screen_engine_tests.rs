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
use vauchi_core::Command;
use vauchi_core::exchange::capability::types::{BiometricType, DeviceCapabilities};

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

fn attempts_status(screen: &ScreenModel) -> Option<(&str, Status)> {
    screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator {
            id, title, status, ..
        } if id == "attempts" => Some((title.as_str(), *status)),
        _ => None,
    })
}

fn biometric_action(screen: &ScreenModel) -> Option<&ScreenAction> {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "unlock_biometric")
}

fn biometric_caps(biometric_type: Option<BiometricType>) -> DeviceCapabilities {
    DeviceCapabilities {
        has_biometrics: true,
        biometric_type,
        ..Default::default()
    }
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
    let (message, status) =
        attempts_status(&screen).expect("a failed attempt must surface a remaining-attempts line");
    assert!(
        message.contains('2'),
        "should show 2 remaining, got: {message}"
    );
    assert!(
        message.contains("remaining"),
        "should mention 'remaining', got: {message}"
    );
    assert_eq!(
        status,
        Status::Warning,
        "remaining attempts is a warning, not a failure"
    );
    let (_, _, validation_error) = lock_input(&screen);
    assert_eq!(
        validation_error, &None,
        "the attempts line is screen status, not a complaint about the field's value"
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

// ── canvas artboard "LockScreen" (ADR-066: Core prepares the whole batch) ──

// @internal
#[test]
fn lock_screen_title_and_subtitle_match_the_canvas() {
    let screen = LockScreenEngine::new(3).current_screen();
    assert_eq!(screen.title, "Vauchi is Locked");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("Enter your password to unlock")
    );
}

// @internal
#[test]
fn lock_screen_opens_with_a_labelled_lock_glyph() {
    let screen = LockScreenEngine::new(3).current_screen();
    let Component::InfoPanel {
        id,
        title,
        items,
        a11y,
        ..
    } = &screen.components[0]
    else {
        panic!(
            "the lock glyph must be the first node, got {:?}",
            screen.components[0]
        );
    };
    assert_eq!(id, "lock_glyph");
    assert_eq!(title, "", "the glyph is decorative: no panel heading");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].icon.as_deref(), Some("lock"));
    assert_eq!(items[0].title, "Locked");
    assert_eq!(items[0].detail, "", "no prose beside the glyph");
    let a11y = a11y.as_ref().expect("the glyph carries its spoken label");
    assert_eq!(a11y.label.as_deref(), Some("Locked"));
    assert_eq!(a11y.role, Some(AccessibilityRole::Image));
}

// @internal
#[test]
fn lock_screen_hides_the_attempts_line_until_an_attempt_fails() {
    let mut engine = LockScreenEngine::new(3);
    assert_eq!(attempts_status(&engine.current_screen()), None);

    engine.record_failed_attempt();
    assert_eq!(
        attempts_status(&engine.current_screen()),
        Some(("2 attempts remaining", Status::Warning))
    );

    engine.record_failed_attempt();
    assert_eq!(
        attempts_status(&engine.current_screen()),
        Some(("1 attempt remaining", Status::Warning))
    );
}

// @internal
#[test]
fn lock_screen_offers_no_biometric_unlock_without_the_capability() {
    let engine = LockScreenEngine::new(3);
    assert_eq!(biometric_action(&engine.current_screen()), None);

    let engine = LockScreenEngine::new(3).with_device_capabilities(&DeviceCapabilities {
        has_biometrics: false,
        biometric_type: Some(BiometricType::FaceId),
        ..Default::default()
    });
    assert_eq!(
        biometric_action(&engine.current_screen()),
        None,
        "a hardware type without the capability flag is not an offer"
    );
}

// @internal
#[test]
fn lock_screen_offers_biometric_unlock_named_after_the_shell_hardware() {
    let cases = [
        (Some(BiometricType::FaceId), "Unlock with Face ID"),
        (Some(BiometricType::Fingerprint), "Unlock with fingerprint"),
        (Some(BiometricType::Iris), "Unlock with biometrics"),
        (None, "Unlock with biometrics"),
    ];
    for (biometric_type, label) in cases {
        let engine =
            LockScreenEngine::new(3).with_device_capabilities(&biometric_caps(biometric_type));
        let screen = engine.current_screen();
        let action = biometric_action(&screen).expect("biometric unlock offered");
        assert_eq!(action.label, label);
        assert_eq!(action.style, ActionStyle::Secondary);
        assert!(action.enabled, "biometrics need no typed password");
        assert!(
            !unlock_enabled(&screen),
            "the password action keeps its own gating"
        );
    }
}

// @internal
#[test]
fn lock_screen_biometric_action_asks_the_shell_to_unlock() {
    let mut engine = LockScreenEngine::new(3)
        .with_device_capabilities(&biometric_caps(Some(BiometricType::FaceId)));
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock_biometric".into(),
    });
    assert_eq!(
        result,
        ActionResult::Commands {
            commands: vec![Command::RequestBiometricUnlock],
        }
    );
}

// @internal
#[test]
fn lock_screen_ignores_a_biometric_press_the_shell_never_offered() {
    let mut engine = LockScreenEngine::new(3);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock_biometric".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "a forged press must not reach the shell, got {result:?}"
    );
}
