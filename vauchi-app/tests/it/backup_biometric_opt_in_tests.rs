// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Backup screen's opt-in biometric check before an export (ADR-080
//! point 7, amended 2026-11-09; vauchi/private#1117). Turning it on shows
//! what it does and does not protect before it takes effect; with it on,
//! an export asks for the OS check, then the session PIN.

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::{BiometricType, DeviceCapabilities};
use vauchi_core::{AuthMode, Command, Event};

const APP_PIN: &str = "app-password-123";
const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-bio-5";
const SECRET: [u8; 32] = [42; 32];

fn backup_screen(biometric_type: Option<BiometricType>) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    vauchi.setup_app_password(APP_PIN).expect("app password");
    assert_eq!(
        vauchi.authenticate(APP_PIN).expect("unlock"),
        AuthMode::Normal
    );
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        can_seal_user_presence: true,
        biometric_type,
        ..Default::default()
    });
    engine.navigate_to(AppScreen::Backup);
    let _ = engine.drain_pending_commands();
    engine
}

fn biometric_device() -> AppEngine {
    backup_screen(Some(BiometricType::Fingerprint))
}

fn press(engine: &mut AppEngine, action_id: &str) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    });
}

fn toggle(engine: &mut AppEngine) {
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "biometric_export".into(),
        item_id: "biometric_toggle".into(),
    });
}

/// The toggle's state as the screen shows it, or `None` when it is absent.
fn toggle_shown(engine: &AppEngine) -> Option<bool> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|component| match component {
            Component::ToggleList { id, items, .. } if id == "biometric_export" => {
                items.first().map(|item| item.selected)
            }
            _ => None,
        })
}

fn shows_explanation(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .components
        .iter()
        .any(|component| matches!(component, Component::Text { id, .. } if id == "biometric_explanation"))
}

fn opted_in(engine: &AppEngine) -> bool {
    engine
        .vauchi()
        .confirms_exports_with_biometrics()
        .expect("the opt-in reads")
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    });
}

fn at_session_pin_step(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .contextual_actions
        .iter()
        .any(|action| action.id == "confirm_pin")
}

fn presence_requested(engine: &mut AppEngine) -> bool {
    engine.drain_pending_commands().into_iter().any(|command| {
        matches!(
            command,
            Command::CreateUserPresenceSecret { .. } | Command::RevealUserPresenceSecret { .. }
        )
    })
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn a_device_without_biometrics_shows_no_toggle() {
    let engine = backup_screen(None);

    assert_eq!(toggle_shown(&engine), None);
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn turning_it_on_explains_first_and_takes_effect_on_confirm() {
    let mut engine = biometric_device();
    assert_eq!(toggle_shown(&engine), Some(false));

    toggle(&mut engine);
    assert!(shows_explanation(&engine), "the explanation comes first");
    assert!(
        !opted_in(&engine),
        "nothing changes before the confirmation"
    );

    press(&mut engine, "turn_on");
    assert!(opted_in(&engine));
    assert_eq!(toggle_shown(&engine), Some(true));
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn backing_out_of_the_explanation_leaves_it_off() {
    let mut engine = biometric_device();
    toggle(&mut engine);

    press(&mut engine, "back");

    assert!(!opted_in(&engine));
    assert_eq!(toggle_shown(&engine), Some(false));
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn turning_it_off_takes_effect_at_once() {
    let mut engine = biometric_device();
    toggle(&mut engine);
    press(&mut engine, "turn_on");

    toggle(&mut engine);

    assert!(!opted_in(&engine));
    assert_eq!(toggle_shown(&engine), Some(false));
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn with_it_on_an_export_asks_for_the_os_check_before_the_session_pin() {
    let mut engine = biometric_device();
    toggle(&mut engine);
    press(&mut engine, "turn_on");
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "backup_rows".into(),
        item_id: "create".into(),
    });
    type_into(&mut engine, "password", BACKUP_PASSWORD);
    press(&mut engine, "continue");
    type_into(&mut engine, "confirm_password", BACKUP_PASSWORD);
    press(&mut engine, "continue");

    assert!(presence_requested(&mut engine), "the OS check comes first");
    assert!(!at_session_pin_step(&engine));

    let _ = engine.handle_hardware_event(Event::UserPresenceSecretCreated {
        secret: SECRET.to_vec().into(),
    });

    assert!(at_session_pin_step(&engine), "then the session PIN");
}
