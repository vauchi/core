// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Setting or turning off the duress PIN, revoking a device and vouching
//! for someone's recovery ask for the session PIN again first, so someone
//! holding the unlocked phone cannot do them (#777, owner decision
//! 2026-10-23). Too many misses lock the app, as at the other PIN steps.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::AuthMode;
use vauchi_core::api::Vauchi;
use vauchi_core::identity::DeviceInfo;

const APP_PIN: &str = "app-password-123";
const DURESS_PIN: &str = "135790";
const OTHER_SEED: [u8; 32] = [9u8; 32];

fn engine_with_password() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password(APP_PIN).unwrap();
    assert_eq!(vauchi.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    AppEngine::new(vauchi)
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn select(engine: &mut AppEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

fn type_into(engine: &mut AppEngine, component_id: &str, text: &str) {
    let mut field = String::new();
    for ch in text.chars() {
        field.push(ch);
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: component_id.into(),
            value: field.clone(),
        });
    }
}

/// The PIN step is told apart by its confirm action: screens such as the
/// duress PIN stamp their own id on every step they show.
fn at_session_pin_step(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .contextual_actions
        .iter()
        .any(|action| action.id == "confirm_pin")
}

fn enter_session_pin(engine: &mut AppEngine, pin: &str) -> ActionResult {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    press(engine, "confirm_pin")
}

fn pin_error(engine: &AppEngine) -> Option<String> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput {
                id,
                validation_error,
                ..
            } if id == "pin" => validation_error.clone(),
            _ => None,
        })
}

fn finish_duress_setup(engine: &mut AppEngine) -> ActionResult {
    engine.navigate_to(AppScreen::DuressPin);
    let _ = select(engine, "duress_actions", "set_up");
    type_into(engine, "pin", DURESS_PIN);
    let _ = press(engine, "continue");
    type_into(engine, "confirm_pin", DURESS_PIN);
    press(engine, "continue")
}

fn with_second_device(engine: &mut AppEngine) {
    let vauchi = engine.vauchi_mut();
    let mut registry = vauchi.identity().unwrap().initial_device_registry();
    registry
        .add_device_unsigned(
            DeviceInfo::derive(&OTHER_SEED, 1, "Laptop".into(), 0).to_registered(&OTHER_SEED),
        )
        .unwrap();
    vauchi
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
}

fn laptop_is_active(engine: &AppEngine) -> bool {
    engine
        .vauchi()
        .list_devices()
        .unwrap()
        .into_iter()
        .find(|device| device.device_name == "Laptop")
        .expect("the laptop is registered")
        .is_active
}

fn confirm_laptop_revoke(engine: &mut AppEngine) -> ActionResult {
    engine.navigate_to(AppScreen::DeviceManagement);
    press(engine, "confirm_revoke_device:1")
}

fn confirm_voucher_for_a_claim(engine: &mut AppEngine) -> ActionResult {
    let claimant: Vauchi = {
        let mut v = Vauchi::in_memory().unwrap();
        v.create_identity("Bob").unwrap();
        v
    };
    let claim = claimant
        .create_recovery_claim_hex_b64(&hex::encode([0xABu8; 32]))
        .unwrap();
    engine.navigate_to(AppScreen::RecoveryHelp);
    let _ = press(engine, "vouch");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "claim_data".into(),
        value: claim,
    });
    let _ = press(engine, "verify_claim");
    press(engine, "create_voucher")
}

fn voucher_shown(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .components
        .iter()
        .any(|component| matches!(component, Component::Text { id, .. } if id == "voucher_data"))
}

// @internal
#[test]
fn setting_the_duress_pin_waits_for_the_session_pin() {
    let mut engine = engine_with_password();

    let _ = finish_duress_setup(&mut engine);

    assert!(at_session_pin_step(&engine));
    assert!(!engine.vauchi().is_duress_enabled().unwrap());
    let _ = enter_session_pin(&mut engine, APP_PIN);
    assert!(engine.vauchi().is_duress_enabled().unwrap());
}

// @internal
#[test]
fn a_wrong_session_pin_sets_no_duress_pin_and_says_so() {
    let mut engine = engine_with_password();
    let _ = finish_duress_setup(&mut engine);

    let _ = enter_session_pin(&mut engine, "not-the-pin");

    assert!(at_session_pin_step(&engine));
    assert!(pin_error(&engine).is_some_and(|e| !e.is_empty()));
    assert!(!engine.vauchi().is_duress_enabled().unwrap());
}

// @internal
#[test]
fn repeated_wrong_session_pins_lock_the_app() {
    let mut engine = engine_with_password();
    let _ = finish_duress_setup(&mut engine);

    for _ in 0..vauchi_app::ui::DEFAULT_LOCK_MAX_ATTEMPTS {
        let _ = enter_session_pin(&mut engine, "not-the-pin");
    }

    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);
    assert!(!engine.vauchi().is_duress_enabled().unwrap());
}

// @internal
#[test]
fn cancelling_the_session_pin_returns_to_the_duress_screen_unchanged() {
    let mut engine = engine_with_password();
    let _ = finish_duress_setup(&mut engine);

    let _ = press(&mut engine, "cancel");

    assert_eq!(engine.current_app_screen(), &AppScreen::DuressPin);
    assert!(!at_session_pin_step(&engine));
    let _ = enter_session_pin(&mut engine, APP_PIN);
    assert!(!engine.vauchi().is_duress_enabled().unwrap());
}

// @internal
#[test]
fn leaving_the_session_pin_step_drops_the_pending_change() {
    let mut engine = engine_with_password();
    let _ = finish_duress_setup(&mut engine);

    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::DuressPin);
    let _ = enter_session_pin(&mut engine, APP_PIN);

    assert!(!engine.vauchi().is_duress_enabled().unwrap());
}

// @internal
#[test]
fn turning_off_the_duress_pin_waits_for_the_session_pin() {
    let mut engine = engine_with_password();
    engine
        .vauchi_mut()
        .setup_duress_password(DURESS_PIN)
        .unwrap();
    engine.navigate_to(AppScreen::DuressPin);
    let _ = select(&mut engine, "duress_actions", "turn_off");

    let _ = press(&mut engine, "confirm_disable");

    assert!(at_session_pin_step(&engine));
    assert!(engine.vauchi().is_duress_enabled().unwrap());
    let _ = enter_session_pin(&mut engine, APP_PIN);
    assert!(!engine.vauchi().is_duress_enabled().unwrap());
}

// @internal
#[test]
fn a_duress_session_confirms_with_its_own_pin() {
    let mut engine = engine_with_password();
    engine
        .vauchi_mut()
        .setup_duress_password(DURESS_PIN)
        .unwrap();
    assert_eq!(
        engine.vauchi_mut().authenticate(DURESS_PIN).unwrap(),
        AuthMode::Duress
    );
    engine.navigate_to(AppScreen::DuressPin);
    let _ = select(&mut engine, "duress_actions", "turn_off");
    let _ = press(&mut engine, "confirm_disable");

    let _ = enter_session_pin(&mut engine, DURESS_PIN);

    assert_eq!(pin_error(&engine), None);
    assert!(!at_session_pin_step(&engine));
}

// @internal
#[test]
fn revoking_a_device_waits_for_the_session_pin() {
    let mut engine = engine_with_password();
    with_second_device(&mut engine);

    let _ = confirm_laptop_revoke(&mut engine);

    assert!(at_session_pin_step(&engine));
    assert!(laptop_is_active(&engine));
    let _ = enter_session_pin(&mut engine, APP_PIN);
    assert!(!laptop_is_active(&engine));
}

// @internal
#[test]
fn a_wrong_session_pin_revokes_nothing() {
    let mut engine = engine_with_password();
    with_second_device(&mut engine);
    let _ = confirm_laptop_revoke(&mut engine);

    let _ = enter_session_pin(&mut engine, "not-the-pin");

    assert!(laptop_is_active(&engine));
}

// @internal
#[test]
fn vouching_for_a_recovery_waits_for_the_session_pin() {
    let mut engine = engine_with_password();

    let _ = confirm_voucher_for_a_claim(&mut engine);

    assert!(at_session_pin_step(&engine));
    let _ = enter_session_pin(&mut engine, APP_PIN);
    assert!(voucher_shown(&engine));
}

// @internal
#[test]
fn a_wrong_session_pin_signs_no_voucher() {
    let mut engine = engine_with_password();
    let _ = confirm_voucher_for_a_claim(&mut engine);

    let _ = enter_session_pin(&mut engine, "not-the-pin");

    assert!(!voucher_shown(&engine));
    assert!(at_session_pin_step(&engine));
}

// @internal
#[test]
fn without_an_app_password_revoking_asks_for_no_pin() {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    with_second_device(&mut engine);

    let _ = confirm_laptop_revoke(&mut engine);

    assert!(!at_session_pin_step(&engine));
    assert!(!laptop_is_active(&engine));
}
