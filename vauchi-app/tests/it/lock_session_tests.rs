// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A locked session refuses every navigation until it is unlocked
//! (vauchi/private#639, owner decision 2026-10-11: security first).
//!
//! Back was one way off the lock screen; tab navigation was another
//! (`problems/2026-08-12-android-app-password-bypass`). Each was closed
//! where it was found, and the next route stayed open. Data reads are not
//! gated by the session (`AuthMode::Unauthenticated` shows real data), so
//! the engine itself holds the lock: entering the lock screen locks the
//! session, only the password or a biometric unlock without a duress PIN
//! unlocks it, and no navigation or surface switch leaves the lock screen
//! in between — deep links, notifications, effects, or a route not yet
//! thought of.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::{AuthMode, Vauchi};
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::{Event, SurfaceId};

const PASSWORD: &str = "pass1234";
const DURESS_PIN: &str = "654321";

fn engine_with_password(duress: bool) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.vauchi_mut().setup_app_password(PASSWORD).unwrap();
    if duress {
        engine
            .vauchi_mut()
            .setup_duress_password(DURESS_PIN)
            .unwrap();
    }
    engine.set_device_capabilities(DeviceCapabilities {
        has_biometrics: true,
        ..DeviceCapabilities::default()
    });
    engine
}

/// Unlocked with the password, on Settings, then sent to the background.
fn locked_after_backgrounding() -> AppEngine {
    let mut engine = engine_with_password(false);
    engine.vauchi_mut().authenticate(PASSWORD).unwrap();
    engine.navigate_to(AppScreen::Settings);
    let _ = engine.dispatch(Event::AppBackgrounded).unwrap();
    assert_eq!(
        engine.current_screen().screen_id,
        "lock_screen",
        "precondition"
    );
    engine
}

fn unlock_with(engine: &mut AppEngine, password: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: password.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
}

// @scenario: security.feature :: App lock with PIN
#[test]
fn a_locked_session_refuses_navigation_to_any_screen() {
    for target in [
        AppScreen::Contacts,
        AppScreen::MyInfo,
        AppScreen::Settings,
        AppScreen::SettingsAdvanced,
        AppScreen::DuressPin,
    ] {
        let mut engine = locked_after_backgrounding();

        engine.navigate_to(target.clone());

        assert_eq!(
            engine.current_screen().screen_id,
            "lock_screen",
            "navigating to {target:?} left the lock screen without the password",
        );
    }
}

// @scenario: security.feature :: App lock with PIN
#[test]
fn a_cold_start_behind_a_password_refuses_navigation() {
    let mut engine = engine_with_password(false);
    engine.bootstrap();
    assert_eq!(
        engine.current_screen().screen_id,
        "lock_screen",
        "precondition"
    );

    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

// @scenario: security.feature :: App lock with PIN
#[test]
fn a_locked_session_refuses_a_surface_switch() {
    let mut engine = locked_after_backgrounding();

    let _ = engine.dispatch(Event::SurfaceActivated {
        surface_id: SurfaceId::new("settings").unwrap(),
    });

    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

/// A shell still holds the surfaces it was shown before the lock (here
/// Settings and its Advanced child); activating one of them must not leave
/// the lock screen.
// @scenario: security.feature :: App lock with PIN
#[test]
fn a_locked_session_refuses_switching_to_a_surface_shown_before_the_lock() {
    for surface in ["settings", "settings_advanced"] {
        let mut engine = engine_with_password(false);
        engine.vauchi_mut().authenticate(PASSWORD).unwrap();
        engine.navigate_to(AppScreen::Settings);
        engine.navigate_to(AppScreen::SettingsAdvanced);
        let _ = engine.initial_commands().unwrap();
        let _ = engine.dispatch(Event::AppBackgrounded).unwrap();
        assert_eq!(
            engine.current_screen().screen_id,
            "lock_screen",
            "precondition"
        );

        let _ = engine.dispatch(Event::SurfaceActivated {
            surface_id: SurfaceId::new(surface).unwrap(),
        });

        assert_eq!(
            engine.current_screen().screen_id,
            "lock_screen",
            "activating {surface} left the lock screen without the password",
        );
    }
}

// @scenario: security.feature :: App lock with PIN
#[test]
fn a_wrong_password_keeps_the_session_locked() {
    let mut engine = locked_after_backgrounding();
    unlock_with(&mut engine, "not-the-password");

    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

// The lock must open for the ways that are meant to open it.

// @internal
#[test]
fn the_password_unlocks_the_session() {
    let mut engine = locked_after_backgrounding();
    unlock_with(&mut engine, PASSWORD);

    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Contacts.screen_id()
    );
}

// @scenario: duress_mode.feature :: Biometric unlock with duress
#[test]
fn the_duress_pin_unlocks_the_session_in_duress_mode() {
    let mut engine = engine_with_password(true);
    engine.bootstrap();
    unlock_with(&mut engine, DURESS_PIN);

    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(engine.vauchi().auth_mode(), AuthMode::Duress);
    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Contacts.screen_id()
    );
}

// @scenario: duress_mode.feature :: Biometric unlock without duress
#[test]
fn a_biometric_unlock_without_duress_unlocks_the_session() {
    let mut engine = engine_with_password(false);
    engine.bootstrap();
    let _ = engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();

    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Contacts.screen_id()
    );
}

// @scenario: duress_mode.feature :: Biometric unlock with duress
#[test]
fn a_biometric_unlock_with_duress_keeps_the_session_locked() {
    let mut engine = engine_with_password(true);
    engine.bootstrap();
    let _ = engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();

    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

// @internal
#[test]
fn an_app_without_a_password_is_never_locked() {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.dispatch(Event::AppBackgrounded).unwrap();

    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Contacts.screen_id()
    );
}
