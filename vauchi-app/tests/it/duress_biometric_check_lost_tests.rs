// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Duress set up with a 6-digit PIN behind biometric unlock, on a device
//! that no longer has biometrics (ADR-032, amended 2026-11-10;
//! vauchi/private#1117 task 10 step 4). Duress keeps working; the real
//! profile's duress settings say the PIN has no biometric check in front
//! of it and offer to move to two real passwords.

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::AuthMode;
use vauchi_core::api::{DuressUnlock, Vauchi};
use vauchi_core::exchange::capability::DeviceCapabilities;

const APP_PASSWORD: &str = "correct-horse-battery-staple-app";
const DURESS_PIN: &str = "654321";
const DURESS_PASSWORD: &str = "plum-violin-orbit-meadow-duress";

fn duress_settings(biometrics_now: bool, unlock_with: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    vauchi
        .setup_app_password(APP_PASSWORD)
        .expect("app password");
    vauchi
        .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .expect("duress set behind biometrics");
    vauchi.authenticate(unlock_with).expect("unlock");
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_biometrics: biometrics_now,
        ..Default::default()
    });
    engine.navigate_to(AppScreen::DuressPin);
    let _ = engine.drain_pending_commands();
    engine
}

fn texts(engine: &AppEngine) -> Vec<String> {
    engine
        .current_screen()
        .components
        .iter()
        .filter_map(|component| match component {
            Component::Text { content, .. } => Some(content.clone()),
            Component::StatusIndicator { title, .. } => Some(title.clone()),
            _ => None,
        })
        .collect()
}

fn offers_two_passwords(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .components
        .iter()
        .any(|component| match component {
            Component::ButtonList { items, .. } => {
                items.iter().any(|item| item.id == "move_to_passwords")
            }
            _ => false,
        })
}

fn warning() -> String {
    get_string(Locale::ENGLISH, "resistance.duress.no_biometric_check")
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    });
}

fn press(engine: &mut AppEngine, action_id: &str) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    });
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn without_biometrics_now_the_settings_say_the_pin_lost_its_check() {
    let engine = duress_settings(false, APP_PASSWORD);

    assert!(texts(&engine).contains(&warning()));
    assert!(offers_two_passwords(&engine));
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn with_biometrics_the_settings_say_nothing_more() {
    let engine = duress_settings(true, APP_PASSWORD);

    assert!(!texts(&engine).contains(&warning()));
    assert!(!offers_two_passwords(&engine));
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_duress_session_shows_no_warning() {
    let engine = duress_settings(false, DURESS_PIN);
    assert_eq!(engine.vauchi().auth_mode(), AuthMode::Duress);

    assert!(!texts(&engine).contains(&warning()));
    assert!(!offers_two_passwords(&engine));
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn moving_to_two_passwords_sets_duress_up_under_the_password_branch() {
    let mut engine = duress_settings(false, APP_PASSWORD);

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "duress_actions".into(),
        item_id: "move_to_passwords".into(),
    });
    type_into(&mut engine, "app_password", APP_PASSWORD);
    press(&mut engine, "continue");
    type_into(&mut engine, "password", DURESS_PASSWORD);
    press(&mut engine, "continue");
    type_into(&mut engine, "confirm_password", DURESS_PASSWORD);
    press(&mut engine, "continue");

    assert_eq!(
        engine.vauchi().duress_unlock().expect("branch loads"),
        Some(DuressUnlock::Passwords)
    );
    assert_eq!(
        engine
            .vauchi_mut()
            .authenticate(DURESS_PASSWORD)
            .expect("unlock"),
        AuthMode::Duress
    );
}
