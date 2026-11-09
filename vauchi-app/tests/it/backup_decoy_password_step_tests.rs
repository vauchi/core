// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A full backup exported from the real profile with a duress PIN set asks
//! for a decoy password, which opens the backup's decoy payload (ADR-080
//! point 7, amended 2026-11-09; vauchi/private#1117). Without a duress PIN,
//! and under duress, the wizard has no such step.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{AuthMode, contact_card::ContactCard};

const APP_PIN: &str = "app-password-123";
const DURESS_PIN: &str = "112233";
const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-real-3";
const DECOY_PASSWORD: &str = "a-separate-decoy-passphrase-for-backups";

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

/// Whether the screen shows the input `id`: the app stamps every wizard
/// screen with the Backup screen's id, so a step is told apart by its field.
fn shows_input(engine: &AppEngine, id: &str) -> bool {
    engine
        .current_screen()
        .components
        .iter()
        .any(|component| matches!(component, Component::TextInput { id: input, .. } if input == id))
}

/// Alice with an app password, a duress PIN when asked for, and one decoy,
/// unlocked with `unlock`.
fn engine(with_duress_pin: bool, unlock: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    vauchi.setup_app_password(APP_PIN).expect("app password");
    if with_duress_pin {
        vauchi
            .setup_duress_password(DURESS_PIN)
            .expect("duress PIN");
        vauchi
            .add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
            .expect("decoy");
    }
    vauchi.authenticate(unlock).expect("unlock");
    AppEngine::new(vauchi)
}

/// Runs the wizard through the backup password and its confirmation.
fn through_the_backup_password(engine: &mut AppEngine) {
    let _ = engine.navigate_to(AppScreen::Backup);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "backup_rows".into(),
        item_id: "create".into(),
    });
    type_into(engine, "password", BACKUP_PASSWORD);
    let _ = press(engine, "continue");
    type_into(engine, "confirm_password", BACKUP_PASSWORD);
    let _ = press(engine, "continue");
}

fn exported(result: ActionResult) -> String {
    match result {
        ActionResult::BackupExportComplete { data } => data,
        other => panic!("expected a finished export, got {other:?}"),
    }
}

fn contact_names(vauchi: &Vauchi) -> Vec<String> {
    vauchi
        .list_contacts()
        .expect("contacts")
        .iter()
        .map(|c| c.display_name().to_string())
        .collect()
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn the_real_profile_with_a_duress_pin_asks_for_a_decoy_password() {
    let mut engine = engine(true, APP_PIN);

    through_the_backup_password(&mut engine);

    assert!(shows_input(&engine, "decoy_password"));
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn the_decoy_password_opens_only_the_decoys() {
    let mut engine = engine(true, APP_PIN);
    through_the_backup_password(&mut engine);
    type_into(&mut engine, "decoy_password", DECOY_PASSWORD);
    let _ = press(&mut engine, "continue");
    assert!(shows_input(&engine, "confirm_decoy_password"));
    type_into(&mut engine, "confirm_decoy_password", DECOY_PASSWORD);
    let _ = press(&mut engine, "continue");
    type_into(&mut engine, "pin", APP_PIN);
    let backup = exported(press(&mut engine, "confirm_pin"));

    let mut decoy = Vauchi::in_memory().expect("fresh install");
    decoy
        .import_full_backup(&backup, DECOY_PASSWORD)
        .expect("the decoy password restores");
    assert_eq!(contact_names(&decoy), vec!["Decoy Dana".to_string()]);
    let mut real = Vauchi::in_memory().expect("fresh install");
    real.import_full_backup(&backup, BACKUP_PASSWORD)
        .expect("the backup password restores");
    assert_eq!(
        real.authenticate(APP_PIN).expect("unlock"),
        AuthMode::Normal
    );
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_mismatched_decoy_confirmation_is_refused() {
    let mut engine = engine(true, APP_PIN);
    through_the_backup_password(&mut engine);
    type_into(&mut engine, "decoy_password", DECOY_PASSWORD);
    let _ = press(&mut engine, "continue");
    type_into(
        &mut engine,
        "confirm_decoy_password",
        "not-the-same-decoy-passphrase",
    );

    let _ = press(&mut engine, "continue");

    let error = engine
        .current_screen()
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput {
                id,
                validation_error,
                ..
            } if id == "confirm_decoy_password" => validation_error.clone(),
            _ => None,
        });
    assert!(error.is_some(), "the mismatch is shown on the confirmation");
    assert!(shows_input(&engine, "confirm_decoy_password"));
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn without_a_duress_pin_and_under_duress_the_wizard_has_no_decoy_step() {
    for (with_duress_pin, unlock) in [(false, APP_PIN), (true, DURESS_PIN)] {
        let mut engine = engine(with_duress_pin, unlock);

        through_the_backup_password(&mut engine);

        assert!(!shows_input(&engine, "decoy_password"));
        assert!(
            engine
                .current_screen()
                .contextual_actions
                .iter()
                .any(|action| action.id == "confirm_pin"),
            "the export goes straight to the session PIN"
        );
    }
}
