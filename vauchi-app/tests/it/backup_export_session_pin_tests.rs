// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every backup export asks for the password of the profile in front, on
//! every install, so the step is the same with or without a duress PIN
//! (ADR-080 point 7, amended 2026-11-09; vauchi/private#1117). It is the
//! session PIN step the credential changes use (#777).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::AuthMode;
use vauchi_core::api::Vauchi;

const APP_PIN: &str = "app-password-123";
const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-pin-7";

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    });
}

fn engine(with_password: bool) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    if with_password {
        vauchi.setup_app_password(APP_PIN).expect("app password");
        assert_eq!(
            vauchi.authenticate(APP_PIN).expect("unlock"),
            AuthMode::Normal
        );
    }
    AppEngine::new(vauchi)
}

/// Runs the backup wizard to its last step and returns what that step
/// answered.
fn request_export(engine: &mut AppEngine) -> Vec<ActionResult> {
    let _ = engine.navigate_to(AppScreen::Backup);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "backup_rows".into(),
        item_id: "create".into(),
    });
    type_into(engine, "password", BACKUP_PASSWORD);
    let first = press(engine, "continue");
    type_into(engine, "confirm_password", BACKUP_PASSWORD);
    let second = press(engine, "continue");
    vec![first, second]
}

fn exported(results: &[ActionResult]) -> Option<String> {
    results.iter().find_map(|result| match result {
        ActionResult::BackupExportComplete { data } => Some(data.clone()),
        _ => None,
    })
}

fn at_session_pin_step(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .contextual_actions
        .iter()
        .any(|action| action.id == "confirm_pin")
}

fn enter_session_pin(engine: &mut AppEngine, pin: &str) -> ActionResult {
    type_into(engine, "pin", pin);
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

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn a_backup_export_waits_for_the_session_pin() {
    let mut engine = engine(true);

    let results = request_export(&mut engine);

    assert_eq!(exported(&results), None);
    assert!(at_session_pin_step(&engine), "the PIN step stands in front");
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn a_wrong_session_pin_exports_nothing() {
    let mut engine = engine(true);
    let _ = request_export(&mut engine);

    let result = enter_session_pin(&mut engine, "not-the-pin");

    assert_eq!(exported(&[result]), None);
    assert!(at_session_pin_step(&engine));
    assert!(pin_error(&engine).is_some(), "the miss is shown");
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn the_session_pin_finishes_the_export() {
    let mut engine = engine(true);
    let _ = request_export(&mut engine);

    let result = enter_session_pin(&mut engine, APP_PIN);

    let backup = exported(&[result]).expect("the export completes");
    let mut restored = Vauchi::in_memory().expect("fresh install");
    restored
        .import_full_backup(&backup, BACKUP_PASSWORD)
        .expect("the backup restores");
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn without_an_app_password_the_export_does_not_ask() {
    let mut engine = engine(false);

    let results = request_export(&mut engine);

    assert!(exported(&results).is_some(), "no PIN to ask for");
}
