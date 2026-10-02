// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Linking a device asks for the PIN again before any link code exists,
//! even in an unlocked app: a completed link hands the new device the
//! identity (#469; owner decision 2026-10-02).

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::AuthMode;
use vauchi_core::api::Vauchi;

const APP_PIN: &str = "app-password-123";
const DURESS_PIN: &str = "135790";

fn unlocked_engine(with_password: bool) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    if with_password {
        engine.vauchi_mut().setup_app_password(APP_PIN).unwrap();
        engine
            .vauchi_mut()
            .setup_duress_password(DURESS_PIN)
            .unwrap();
        assert_eq!(
            engine.vauchi_mut().authenticate(APP_PIN).unwrap(),
            AuthMode::Normal
        );
    }
    engine
}

fn enter_pin(engine: &mut AppEngine, pin: &str) -> String {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_pin".into(),
    });
    engine.current_screen().screen_id
}

// @scenario: device_management :: Generate device linking code
// @internal
#[test]
fn linking_asks_for_the_pin_before_any_link_code() {
    let mut engine = unlocked_engine(true);
    let screen = engine.navigate_to(AppScreen::DeviceLinking);
    assert_eq!(screen.screen_id, "link_confirm_pin");
}

// @scenario: device_management :: Generate device linking code
// @internal
#[test]
fn the_pin_that_unlocked_the_app_starts_the_link() {
    let mut engine = unlocked_engine(true);
    engine.navigate_to(AppScreen::DeviceLinking);
    assert_eq!(enter_pin(&mut engine, APP_PIN), "link_qr_pending");
}

// @scenario: device_management :: Generate device linking code
// @internal
#[test]
fn a_wrong_pin_keeps_asking_and_never_changes_the_session() {
    let mut engine = unlocked_engine(true);
    engine.navigate_to(AppScreen::DeviceLinking);
    assert_eq!(enter_pin(&mut engine, "wrong"), "link_confirm_pin");
    assert_eq!(enter_pin(&mut engine, DURESS_PIN), "link_confirm_pin");
    let wrong = get_string(Locale::English, "lock_screen.wrong_password");
    assert!(
        format!("{:?}", engine.current_screen()).contains(&wrong),
        "the screen says the PIN was wrong"
    );
    assert_eq!(engine.vauchi().auth_mode(), AuthMode::Normal);
}

// Without a limit, someone holding the unlocked phone could try PINs here
// for ever; after as many failures as the lock screen allows, lock the app.
// @scenario: device_management :: Generate device linking code
// @internal
#[test]
fn five_wrong_pins_lock_the_app() {
    let mut engine = unlocked_engine(true);
    engine.navigate_to(AppScreen::DeviceLinking);
    for _ in 0..5 {
        enter_pin(&mut engine, "wrong");
    }
    assert!(matches!(engine.current_app_screen(), AppScreen::Lock));
}

// @scenario: device_management :: Generate device linking code
// @internal
#[test]
fn without_an_app_password_the_link_starts_directly() {
    let mut engine = unlocked_engine(false);
    let screen = engine.navigate_to(AppScreen::DeviceLinking);
    assert_eq!(screen.screen_id, "link_qr_pending");
}
