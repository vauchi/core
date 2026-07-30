// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine settings toggle persistence and lock screen PIN verification tests.

use crate::common;

use common::app_engine_helpers::{engine_with_password, enter_pin, find_settings_toggle};
use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

// ── settings toggle persistence tests (HIGH-4) ──────────────────────

// @internal
#[test]
fn settings_toggle_persists_after_navigate_away_and_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(
        find_settings_toggle(&screen, "privacy_notifications", "delivery_receipts"),
        "delivery_receipts should default to enabled"
    );

    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy_notifications".into(),
        item_id: "delivery_receipts".into(),
    });
    match &result {
        ActionResult::UpdateScreen(s) => {
            assert!(
                !find_settings_toggle(s, "privacy_notifications", "delivery_receipts"),
                "delivery_receipts should be disabled after toggle"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }

    engine.navigate_to(AppScreen::MyInfo);

    // Invalidate Settings cache to force fresh engine from vauchi.config()
    engine.invalidate_screen(&AppScreen::Settings);

    // Navigate back to Settings — toggle should still be off
    let restored = engine.navigate_to(AppScreen::Settings);
    assert!(
        !find_settings_toggle(&restored, "privacy_notifications", "delivery_receipts"),
        "delivery_receipts toggle should persist after navigating away and back (even with cache invalidated)"
    );
}

// @internal
#[test]
fn settings_toggle_suppress_presence_persists() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(
        !find_settings_toggle(&screen, "privacy_notifications", "suppress_presence"),
        "suppress_presence should default to disabled"
    );

    // Intermediate step: toggle on — persistence asserted after navigate-away-and-back
    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy_notifications".into(),
        item_id: "suppress_presence".into(),
    });

    engine.navigate_to(AppScreen::MyInfo);
    engine.invalidate_screen(&AppScreen::Settings);

    // Navigate back — should still be on
    let restored = engine.navigate_to(AppScreen::Settings);
    assert!(
        find_settings_toggle(&restored, "privacy_notifications", "suppress_presence"),
        "suppress_presence toggle should persist after navigating away and back"
    );
}

// @internal
#[test]
fn settings_emergency_wipe_navigates_to_shred() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Settings);

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

// @internal
#[test]
fn duress_pin_screen_renders_with_defaults() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DuressPin);
    assert_eq!(screen.screen_id, "duress_pin");
    assert_eq!(screen.title, "Duress PIN");
}

// @scenario: duress_password :: Duress PIN setup persists through AppEngine
// @internal
#[test]
fn duress_pin_setup_persists_via_handle_completion() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("123456").unwrap();
    // A recipient must exist + be chosen for duress setup to complete.
    let bob = Contact::from_exchange(
        [7u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let bob_id = bob.id().to_string();
    vauchi.add_contact(bob).unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::DuressPin);

    // Step 1: Press "configure" on overview
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });

    // Step 2: Enter PIN digits
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "654321".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Step 3: Confirm PIN
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_pin".into(),
        value: "654321".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Choose Bob as the alert recipient (required before Save is enabled).
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "recipients".into(),
        item_id: bob_id.clone(),
    });

    // Step 4: Save (triggers Complete → handle_completion)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    // Should navigate back (not show error alert)
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "save should navigate back, got {:?}",
        result
    );

    let vauchi = engine.vauchi();
    assert!(
        vauchi.is_duress_enabled().unwrap(),
        "duress must be enabled after completing setup flow"
    );
}

// @scenario: duress_password :: Disabling duress PIN persists
// @internal
#[test]
fn duress_pin_disable_persists_via_handle_completion() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("123456").unwrap();
    vauchi.setup_duress_password("654321").unwrap();
    assert!(vauchi.is_duress_enabled().unwrap());
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::DuressPin);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_disable".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "confirm_disable should navigate back, got {:?}",
        result
    );

    let vauchi = engine.vauchi();
    assert!(
        !vauchi.is_duress_enabled().unwrap(),
        "duress must be disabled after confirm_disable"
    );
}

// ── change password flow tests ───────────────────────────────────────

// @internal
#[test]
fn settings_change_password_navigates_to_change_password_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("old-pin-1234").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Settings);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "security_backup".into(),
        item_id: "change_password".into(),
    });
    let ActionResult::NavigateTo(screen) = result else {
        panic!("expected NavigateTo, got {result:?}");
    };
    assert_eq!(screen.screen_id, "change_password");
    assert_eq!(engine.current_app_screen(), &AppScreen::ChangePassword);
}

// @internal
#[test]
fn change_password_screen_renders_three_password_fields() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("old-pin-1234").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ChangePassword);
    let component_ids: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            vauchi_app::ui::Component::TextInput { id, input_type, .. } => {
                assert_eq!(
                    *input_type,
                    vauchi_app::ui::InputType::Password,
                    "every input on ChangePassword must be InputType::Password"
                );
                Some(id.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(component_ids.contains(&"current_password"));
    assert!(component_ids.contains(&"new_password"));
    assert!(component_ids.contains(&"confirm_password"));
}

// @internal
#[test]
fn change_password_submit_persists_via_handle_completion() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("old-pin-1234").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::ChangePassword);

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "current_password".into(),
        value: "old-pin-1234".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "new_password".into(),
        value: "new-pin-9876".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "new-pin-9876".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "submit should navigate back on success, got {result:?}"
    );

    // Storage was rotated — old fails, new succeeds.
    let vauchi = engine.vauchi_mut();
    assert!(vauchi.authenticate("old-pin-1234").is_err());
    assert!(vauchi.authenticate("new-pin-9876").is_ok());
}

// @internal
#[test]
fn change_password_wrong_current_shows_alert_storage_unchanged() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("old-pin-1234").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::ChangePassword);

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "current_password".into(),
        value: "WRONG-current".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "new_password".into(),
        value: "new-pin-9876".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "new-pin-9876".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    let ActionResult::ShowAlert { message, .. } = &result else {
        panic!("wrong current password should ShowAlert, got {result:?}");
    };
    assert!(
        message.contains("change password"),
        "change-mode error must use the 'change password' verb, got {message:?}"
    );

    // Storage unchanged — old still authenticates.
    let vauchi = engine.vauchi_mut();
    assert!(vauchi.authenticate("old-pin-1234").is_ok());
    assert!(vauchi.authenticate("new-pin-9876").is_err());
}

// ── set-password (setup mode: no password configured yet) ───────────

// @internal
#[test]
fn set_password_screen_omits_current_field_when_no_password() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    // No setup_app_password — identity has no password yet.
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ChangePassword);
    assert_eq!(
        screen.title, "Set Password",
        "setup mode should title the screen 'Set Password', not 'Change Password'"
    );
    let input_ids: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            vauchi_app::ui::Component::TextInput { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !input_ids.contains(&"current_password"),
        "setup mode must not ask for a current password (there is none), got {input_ids:?}"
    );
    assert!(input_ids.contains(&"new_password"));
    assert!(input_ids.contains(&"confirm_password"));
}

// @internal
#[test]
fn set_password_submit_persists_via_setup_app_password() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    assert!(!vauchi.is_password_enabled().unwrap());
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::ChangePassword);

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "new_password".into(),
        value: "first-pin-1234".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "first-pin-1234".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "setup submit should navigate back on success, got {result:?}"
    );

    let vauchi = engine.vauchi_mut();
    assert!(
        vauchi.is_password_enabled().unwrap(),
        "an app password should now be configured"
    );
    assert!(vauchi.authenticate("first-pin-1234").is_ok());
    assert!(vauchi.authenticate("wrong-pin").is_err());
}

// @internal
#[test]
fn set_password_mismatch_disables_submit() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::ChangePassword);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "new_password".into(),
        value: "first-pin-1234".into(),
    });
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "different".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("expected UpdateScreen, got {result:?}");
    };
    let submit = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "submit")
        .expect("submit action");
    assert!(
        !submit.enabled,
        "setup-mode submit must stay disabled while new != confirm"
    );
}

// @internal
#[test]
fn set_password_cancel_leaves_no_password_configured() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::ChangePassword);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "new_password".into(),
        value: "first-pin-1234".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "first-pin-1234".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));
    assert!(
        !engine.vauchi_mut().is_password_enabled().unwrap(),
        "cancel must not configure a password"
    );
}

// @internal
#[test]
fn change_password_screen_shows_change_mode_after_setup() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // First visit: setup mode, set the first password.
    engine.navigate_to(AppScreen::ChangePassword);
    for (id, v) in [
        ("new_password", "first-pin-1234"),
        ("confirm_password", "first-pin-1234"),
    ] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: id.into(),
            value: v.into(),
        });
    }
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    // Re-open: now a password exists → the screen must render in change mode
    // (the engine cache must not pin the stale setup-mode engine).
    let screen = engine.navigate_to(AppScreen::ChangePassword);
    assert_eq!(screen.title, "Change Password");
    let ids: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            vauchi_app::ui::Component::TextInput { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        ids.contains(&"current_password"),
        "change mode must re-show the current-password field, got {ids:?}"
    );
}

// @internal
#[test]
fn change_password_mismatch_disables_submit() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("old-pin-1234").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::ChangePassword);

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "current_password".into(),
        value: "old-pin-1234".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "new_password".into(),
        value: "new-pin-9876".into(),
    });
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_password".into(),
        value: "different".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("expected UpdateScreen, got {result:?}");
    };
    let submit = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "submit")
        .expect("submit action present");
    assert!(
        !submit.enabled,
        "submit must be disabled while new != confirm"
    );
}

// ── lock screen password verification tests (CRIT-3) ─────────────────

// @internal
#[test]
fn lock_screen_wrong_pin_stays_locked() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

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

// @internal
#[test]
fn lock_screen_correct_pin_unlocks() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

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

// @internal
#[test]
fn lock_screen_empty_pin_does_not_unlock() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

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

// @internal
#[test]
fn lock_screen_tracks_failed_attempts() {
    let mut engine = engine_with_password("123456");

    for _ in 0..2 {
        enter_pin(&mut engine, "000000");
        // Intermediate step: trigger failed attempt — attempt count asserted below
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "unlock".into(),
        });
        // Clear PIN for next attempt — navigate back to lock to get fresh engine
        // We need to clear the entered PIN for the next attempt.
    }

    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Lock,
        "should remain locked after failed attempts"
    );

    let screen = engine.current_screen();
    let has_validation_error = screen.components.iter().any(|c| {
        matches!(
            c,
            vauchi_app::ui::Component::TextInput {
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

// @internal
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
