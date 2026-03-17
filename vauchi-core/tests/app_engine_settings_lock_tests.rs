// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine settings toggle persistence and lock screen PIN verification tests.

mod common;

use common::app_engine_helpers::{engine_with_password, enter_pin, find_settings_toggle};
use vauchi_core::api::Vauchi;
use vauchi_core::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};

// ── settings toggle persistence tests (HIGH-4) ──────────────────────

#[test]
fn settings_toggle_persists_after_navigate_away_and_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Settings
    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(
        find_settings_toggle(&screen, "privacy", "delivery_receipts"),
        "delivery_receipts should default to enabled"
    );

    // Toggle delivery_receipts off
    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "delivery_receipts".into(),
    });
    match &result {
        ActionResult::UpdateScreen(s) => {
            assert!(
                !find_settings_toggle(s, "privacy", "delivery_receipts"),
                "delivery_receipts should be disabled after toggle"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }

    // Navigate away to Home
    engine.navigate_to(AppScreen::MyInfo);

    // Invalidate Settings cache to force fresh engine from vauchi.config()
    engine.invalidate_screen(&AppScreen::Settings);

    // Navigate back to Settings — toggle should still be off
    let restored = engine.navigate_to(AppScreen::Settings);
    assert!(
        !find_settings_toggle(&restored, "privacy", "delivery_receipts"),
        "delivery_receipts toggle should persist after navigating away and back (even with cache invalidated)"
    );
}

#[test]
fn settings_toggle_suppress_presence_persists() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Settings
    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(
        !find_settings_toggle(&screen, "privacy", "suppress_presence"),
        "suppress_presence should default to disabled"
    );

    // Intermediate step: toggle on — persistence asserted after navigate-away-and-back
    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "suppress_presence".into(),
    });

    // Navigate away and invalidate
    engine.navigate_to(AppScreen::MyInfo);
    engine.invalidate_screen(&AppScreen::Settings);

    // Navigate back — should still be on
    let restored = engine.navigate_to(AppScreen::Settings);
    assert!(
        find_settings_toggle(&restored, "privacy", "suppress_presence"),
        "suppress_presence toggle should persist after navigating away and back"
    );
}

#[test]
fn settings_emergency_wipe_navigates_to_shred() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Settings
    engine.navigate_to(AppScreen::Settings);

    // Select emergency_wipe from the danger group
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "shred_warning",
                "emergency_wipe should navigate to EmergencyShred screen"
            );
        }
        other => panic!("Expected NavigateTo shred_warning, got {:?}", other),
    }
}

#[test]
fn duress_pin_screen_renders_with_defaults() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DuressPin);
    assert_eq!(screen.screen_id, "duress_overview");
    assert_eq!(screen.title, "Duress PIN");
}

// ── lock screen password verification tests (CRIT-3) ─────────────────

#[test]
fn lock_screen_wrong_pin_stays_locked() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Enter wrong PIN and press unlock
    enter_pin(&mut engine, "999999");
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    // Should NOT navigate to Home — should show error or stay on lock
    assert_ne!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "wrong PIN must NOT unlock the app"
    );
    assert!(
        !matches!(result, ActionResult::NavigateTo(_)),
        "wrong PIN should not produce NavigateTo, got {:?}",
        result
    );
}

#[test]
fn lock_screen_correct_pin_unlocks() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Enter correct PIN and press unlock
    enter_pin(&mut engine, "123456");
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    // Should navigate to Home (T-1: verify screen_id)
    let ActionResult::NavigateTo(screen) = result else {
        panic!("correct PIN should navigate to Home, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "my_info",
        "correct PIN should navigate to home screen"
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "should be on Home after correct PIN"
    );
}

#[test]
fn lock_screen_empty_pin_does_not_unlock() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Press unlock without entering any PIN
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    assert_ne!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "empty PIN must NOT unlock the app"
    );
    assert!(
        !matches!(result, ActionResult::NavigateTo(_)),
        "empty PIN should not produce NavigateTo, got {:?}",
        result
    );
}

#[test]
fn lock_screen_tracks_failed_attempts() {
    let mut engine = engine_with_password("123456");

    // Enter wrong PIN twice
    for _ in 0..2 {
        enter_pin(&mut engine, "000000");
        // Intermediate step: trigger failed attempt — attempt count asserted below
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "unlock".into(),
        });
        // Clear PIN for next attempt — navigate back to lock to get fresh engine
        // Actually LockScreenEngine should still be active, but PIN persists.
        // We need to clear the entered PIN for the next attempt.
        // The lock screen should show remaining attempts in the validation error.
    }

    // Should still be on Lock screen
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Lock,
        "should remain locked after failed attempts"
    );

    // The screen should show attempt tracking info
    let screen = engine.current_screen();
    let has_validation_error = screen.components.iter().any(|c| {
        matches!(
            c,
            vauchi_core::ui::Component::PinInput {
                validation_error: Some(_),
                ..
            }
        )
    });
    assert!(
        has_validation_error,
        "lock screen should show remaining attempts after failures"
    );
}

#[test]
fn lock_screen_correct_pin_after_failed_attempt_unlocks() {
    let mut engine = engine_with_password("123456");

    // First attempt: wrong PIN — intermediate step; correct PIN unlock asserted below
    enter_pin(&mut engine, "000000");
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Second attempt: correct PIN
    enter_pin(&mut engine, "123456");
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    let ActionResult::NavigateTo(screen) = result else {
        panic!("correct PIN after failed attempt should unlock, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "my_info",
        "correct PIN after failed attempt should navigate to home"
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}
