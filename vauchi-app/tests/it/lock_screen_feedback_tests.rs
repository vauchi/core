// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lock screen feedback (#429, owner decisions 2026-09-28): a wrong
//! password is reported on the password field itself — "Wrong password."
//! with the attempts left — so it is identified in text and read with the
//! field (WCAG 3.3.1), instead of a separate status line that never said
//! the password was wrong. The "Locked" chip that repeated the title is
//! gone.

use vauchi_app::ui::{ActionResult, Component, LockScreenEngine, UserAction, WorkflowEngine};

fn type_password(engine: &mut LockScreenEngine, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: value.into(),
    });
}

fn fail(engine: &mut LockScreenEngine) -> ActionResult {
    type_password(engine, "not-the-password");
    engine.handle_action(UserAction::ActionPressed {
        action_id: "auth_failed".into(),
    })
}

fn field_error(result: &ActionResult) -> Option<(String, String)> {
    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => Some((component_id.clone(), message.clone())),
        _ => None,
    }
}

// @internal
#[test]
fn the_lock_screen_does_not_repeat_the_title_as_a_locked_chip() {
    let engine = LockScreenEngine::new(3);
    let screen = engine.current_screen();

    assert!(
        !screen.components.iter().any(|c| matches!(
            c,
            Component::InfoPanel { id, .. } if id == "lock_glyph"
        )),
        "the Locked chip repeats the title to screen readers"
    );
}

// @internal
#[test]
fn a_wrong_password_is_reported_on_the_field_with_attempts_left() {
    let mut engine = LockScreenEngine::new(3);

    let first = fail(&mut engine);
    assert_eq!(
        field_error(&first),
        Some((
            "pin".to_string(),
            "Wrong password. 2 attempts remaining".to_string()
        ))
    );

    let second = fail(&mut engine);
    assert_eq!(
        field_error(&second).map(|(_, m)| m).as_deref(),
        Some("Wrong password. 1 attempt remaining")
    );
}

// @internal
#[test]
fn the_attempts_are_said_once_not_in_a_second_status_line() {
    let mut engine = LockScreenEngine::new(3);
    let _ = fail(&mut engine);

    assert!(
        !engine.current_screen().components.iter().any(|c| matches!(
            c,
            Component::StatusIndicator { id, .. } if id == "attempts"
        )),
        "the attempts count belongs to the field's error, not a separate line"
    );
}
