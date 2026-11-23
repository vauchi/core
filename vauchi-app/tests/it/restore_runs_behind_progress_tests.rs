// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A restore decrypts and imports the whole backup: about 90 s for 10k
//! contacts on a Pixel 3a. It ran inside the call that handled the submit,
//! so the password screen stayed up, unchanged and tappable, until it was
//! done (vauchi/private#273). The submit now shows "Restoring backup…"
//! with nothing to tap and asks for an immediate wakeup; the import runs
//! on that wakeup.

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::{Command, Event, Vauchi};

const PASSWORD: &str = "correct horse battery staple";

fn make_backup(password: &str, name: &str) -> Vec<u8> {
    let mut donor = Vauchi::in_memory().unwrap();
    donor.create_identity(name).unwrap();
    donor.export_full_backup(password).unwrap().into_bytes()
}

/// A fresh install on the backup password page, `typed` in the field.
fn on_the_backup_password_page(typed: &str) -> AppEngine {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let _ = engine.navigate_to(AppScreen::Onboarding);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "load_backup".into(),
    });
    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: make_backup(PASSWORD, "Bob"),
        filename: "bob-backup.vauchi".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "backup_password".into(),
        value: typed.into(),
    });
    engine
}

fn submit(engine: &mut AppEngine) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_backup_password".into(),
    });
}

fn replaced_surface_ids(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

// @internal
#[test]
fn submitting_the_password_shows_restoring_with_nothing_to_tap_and_imports_nothing_yet() {
    let mut engine = on_the_backup_password_page(PASSWORD);

    submit(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(
        screen.title,
        get_string(Locale::ENGLISH, "backup.wizard.restoring_title")
    );
    assert_eq!(screen.contextual_actions.len(), 0);
    assert!(
        !engine.vauchi().has_identity(),
        "the import waits for the wakeup"
    );
}

// @internal
#[test]
fn submitting_the_password_asks_for_a_wakeup_at_once() {
    let mut engine = on_the_backup_password_page(PASSWORD);

    submit(&mut engine);

    let delays: Vec<u32> = engine
        .drain_pending_commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::ScheduleWakeup { delay_millis, .. } => Some(delay_millis),
            _ => None,
        })
        .collect();
    assert_eq!(delays, vec![0]);
}

// @internal
#[test]
fn the_wakeup_runs_the_restore_and_shows_the_restored_app() {
    let mut engine = on_the_backup_password_page(PASSWORD);
    submit(&mut engine);

    let commands = engine.dispatch(Event::WakeupFired).expect("wakeup");

    assert!(engine.vauchi().has_identity());
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
    assert!(
        replaced_surface_ids(&commands).contains(&"my_info".to_owned()),
        "the restored app is presented: {:?}",
        replaced_surface_ids(&commands)
    );
}

// @internal
#[test]
fn a_wrong_password_ends_in_an_alert_on_the_wakeup_and_imports_nothing() {
    let mut engine = on_the_backup_password_page("wrong password");
    submit(&mut engine);

    let commands = engine.dispatch(Event::WakeupFired).expect("wakeup");

    assert!(!engine.vauchi().has_identity());
    let alerts: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            Command::PresentAlert { alert } => Some(alert.title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        alerts,
        vec![get_string(Locale::ENGLISH, "backup.restore_error_title").as_str()]
    );
}
