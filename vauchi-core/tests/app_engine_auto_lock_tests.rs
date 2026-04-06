// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for auto-lock on app background (C1 App Security).
//!
//! When the app goes to the background and a password is set,
//! the next foreground event should require re-authentication.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

/// Helper: enter PIN and submit on the lock screen.
fn unlock(engine: &mut AppEngine, pin: &str) {
    let text_result = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    assert!(
        matches!(text_result, ActionResult::UpdateScreen(_)),
        "PIN text entry should update screen, got {text_result:?}"
    );
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "correct PIN should navigate away from lock, got {result:?}"
    );
}

/// Helper: create an AppEngine with password enabled and unlocked.
fn unlocked_engine_with_password() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("123456").unwrap();
    let mut engine = AppEngine::new(vauchi);
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Lock,
        "password-enabled engine should start on Lock"
    );
    unlock(&mut engine, "123456");
    assert_ne!(
        engine.current_app_screen(),
        &AppScreen::Lock,
        "should be unlocked after correct PIN"
    );
    engine
}

// @scenario: app_security :: App locks on background when password is set
#[test]
fn backgrounding_with_password_locks_on_resume() {
    let mut engine = unlocked_engine_with_password();

    // Navigate to some screen
    engine.navigate_to(AppScreen::Settings);
    assert_eq!(engine.current_app_screen(), &AppScreen::Settings);

    // App goes to background and comes back
    let lock_screen = engine.handle_app_backgrounded();
    assert!(
        lock_screen.is_some(),
        "should return lock screen when password is set"
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Lock,
        "should be on Lock screen after background"
    );
}

// @scenario: app_security :: App does not lock without password
#[test]
fn backgrounding_without_password_does_not_lock() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Settings);
    let lock_screen = engine.handle_app_backgrounded();
    assert!(
        lock_screen.is_none(),
        "should NOT lock when no password is set"
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Settings,
        "should stay on current screen"
    );
}

// @scenario: app_security :: Already locked app stays locked on background
#[test]
fn backgrounding_while_already_locked_stays_locked() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("123456").unwrap();
    let mut engine = AppEngine::new(vauchi);
    // Engine starts on Lock — don't unlock
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    let lock_screen = engine.handle_app_backgrounded();
    assert!(
        lock_screen.is_none(),
        "should return None when already on Lock (no-op)"
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);
}

// @scenario: app_security :: Onboarding is not interrupted by background lock
#[test]
fn backgrounding_during_onboarding_does_not_lock() {
    let vauchi = Vauchi::in_memory().unwrap();
    // No identity = onboarding
    let mut engine = AppEngine::new(vauchi);
    assert_eq!(engine.current_app_screen(), &AppScreen::Onboarding);

    let lock_screen = engine.handle_app_backgrounded();
    assert!(
        lock_screen.is_none(),
        "should not lock during onboarding (no identity yet)"
    );
}
