// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress setup offers the branch the device allows (ADR-032, amended
//! 2026-11-10; vauchi/private#1117 task 10 step 2). With biometric unlock
//! it asks for a 6-digit PIN, as before. Without it, it asks for the
//! current app password, then a duress password and its confirmation,
//! both under the backup password rule; a weak app password is named, and
//! the screen offers to change it.

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::AuthMode;
use vauchi_core::api::{DuressUnlock, Vauchi};
use vauchi_core::exchange::capability::{BiometricType, DeviceCapabilities};

const STRONG_APP_PASSWORD: &str = "correct-horse-battery-staple-app";
const STRONG_DURESS_PASSWORD: &str = "plum-violin-orbit-meadow-duress";

fn duress_screen(app_password: &str, biometrics: bool) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    vauchi
        .setup_app_password(app_password)
        .expect("app password");
    assert_eq!(
        vauchi.authenticate(app_password).expect("unlock"),
        AuthMode::Normal
    );
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_biometrics: biometrics,
        biometric_type: biometrics.then_some(BiometricType::Fingerprint),
        ..Default::default()
    });
    engine.navigate_to(AppScreen::DuressPin);
    let _ = engine.drain_pending_commands();
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "duress_actions".into(),
        item_id: "set_up".into(),
    });
    engine
}

fn t(key: &str) -> String {
    get_string(Locale::ENGLISH, key)
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    });
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn shows_pin_input(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .components
        .iter()
        .any(|component| matches!(component, Component::PinInput { .. }))
}

/// The text input `id` on the current screen, with its validation error.
fn text_input(engine: &AppEngine, id: &str) -> Option<Option<String>> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput {
                id: input_id,
                validation_error,
                ..
            } if input_id == id => Some(validation_error.clone()),
            _ => None,
        })
}

fn offers_to_change_the_app_password(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .contextual_actions
        .iter()
        .any(|action| action.id == "change_app_password")
}

fn enter_password_branch(engine: &mut AppEngine, app_password: &str, duress: &str, confirm: &str) {
    type_into(engine, "app_password", app_password);
    let _ = press(engine, "continue");
    type_into(engine, "password", duress);
    let _ = press(engine, "continue");
    type_into(engine, "confirm_password", confirm);
    let _ = press(engine, "continue");
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn with_biometric_unlock_the_setup_asks_for_a_six_digit_pin() {
    let engine = duress_screen("2468", true);

    assert!(shows_pin_input(&engine));
    assert_eq!(text_input(&engine, "app_password"), None);
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn without_biometric_unlock_the_setup_asks_for_the_app_password_first() {
    let engine = duress_screen(STRONG_APP_PASSWORD, false);

    assert!(!shows_pin_input(&engine));
    assert_eq!(text_input(&engine, "app_password"), Some(None));
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn two_real_passwords_set_duress_up_under_the_password_branch() {
    let mut engine = duress_screen(STRONG_APP_PASSWORD, false);

    enter_password_branch(
        &mut engine,
        STRONG_APP_PASSWORD,
        STRONG_DURESS_PASSWORD,
        STRONG_DURESS_PASSWORD,
    );

    assert_eq!(
        engine.vauchi().duress_unlock().expect("branch loads"),
        Some(DuressUnlock::Passwords)
    );
    assert_eq!(
        engine
            .vauchi_mut()
            .authenticate(STRONG_DURESS_PASSWORD)
            .expect("unlock"),
        AuthMode::Duress
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn a_weak_app_password_is_named_and_the_screen_offers_to_change_it() {
    let mut engine = duress_screen("2468", false);
    type_into(&mut engine, "app_password", "2468");

    let _ = press(&mut engine, "continue");

    assert_eq!(
        text_input(&engine, "app_password"),
        Some(Some(t("resistance.duress.error_weak_app_password")))
    );
    assert!(offers_to_change_the_app_password(&engine));

    let _ = press(&mut engine, "change_app_password");

    assert_eq!(engine.current_app_screen(), &AppScreen::ChangePassword);
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn a_weak_duress_password_is_refused_on_its_step() {
    let mut engine = duress_screen(STRONG_APP_PASSWORD, false);
    type_into(&mut engine, "app_password", STRONG_APP_PASSWORD);
    let _ = press(&mut engine, "continue");
    type_into(&mut engine, "password", "654321");

    let _ = press(&mut engine, "continue");

    assert_eq!(
        text_input(&engine, "password"),
        Some(Some(t("resistance.duress.error_weak_password")))
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn a_mismatched_confirmation_is_refused() {
    let mut engine = duress_screen(STRONG_APP_PASSWORD, false);

    enter_password_branch(
        &mut engine,
        STRONG_APP_PASSWORD,
        STRONG_DURESS_PASSWORD,
        "plum-violin-orbit-meadow-other",
    );

    assert_eq!(
        text_input(&engine, "confirm_password"),
        Some(Some(t("resistance.duress.error_password_mismatch")))
    );
    assert!(!engine.vauchi().is_duress_enabled().expect("duress state"));
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn a_wrong_app_password_sets_nothing_and_says_so() {
    let mut engine = duress_screen(STRONG_APP_PASSWORD, false);

    enter_password_branch(
        &mut engine,
        "a-different-but-strong-passphrase",
        STRONG_DURESS_PASSWORD,
        STRONG_DURESS_PASSWORD,
    );

    assert!(!engine.vauchi().is_duress_enabled().expect("duress state"));
    assert_eq!(
        text_input(&engine, "app_password"),
        Some(Some(t("resistance.duress.error_wrong_app_password"))),
        "the setup returns to the app password step and names the error"
    );
}
