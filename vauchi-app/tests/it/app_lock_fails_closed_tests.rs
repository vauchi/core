// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The app password is the protection a person chose. Three places decided
//! whether to show the lock with `is_password_enabled().unwrap_or(false)`,
//! so a password record that could not be read (an SQL or decryption error,
//! a hash of the wrong length) counted as "no password": the app started
//! unlocked and never locked on backgrounding (vauchi/private#1370). An
//! unreadable record now keeps the lock.

use super::common::app_engine_helpers::enter_pin;
use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

const PASSWORD: &str = "correct horse battery staple";

fn make_password_record_unreadable(vauchi: &Vauchi) {
    vauchi
        .storage()
        .connection()
        .execute_batch("UPDATE identity SET password_hash_encrypted = x'010203' WHERE id = 1")
        .expect("corrupt the stored password hash");
    assert!(
        vauchi.is_password_enabled().is_err(),
        "the record must now be unreadable"
    );
}

/// A Vauchi with an identity and a password whose record cannot be read.
fn vauchi_with_unreadable_password() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password(PASSWORD).unwrap();
    make_password_record_unreadable(&vauchi);
    vauchi
}

/// An app with a password, opened with it and on My Card.
fn unlocked_app() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password(PASSWORD).unwrap();
    vauchi.authenticate(PASSWORD).unwrap();
    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
    engine
}

// @internal
#[test]
fn an_unreadable_password_record_starts_the_app_on_the_lock_screen() {
    let engine = AppEngine::new(vauchi_with_unreadable_password());

    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);
}

// @internal
#[test]
fn backgrounding_with_an_unreadable_password_record_still_locks() {
    let mut engine = unlocked_app();
    make_password_record_unreadable(engine.vauchi());

    let locked = engine.handle_app_backgrounded();

    assert_eq!(
        locked.map(|screen| screen.screen_id),
        Some("lock_screen".to_owned())
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);
}

// @internal
#[test]
fn the_lock_does_not_open_while_the_password_record_is_unreadable() {
    let mut engine = AppEngine::new(vauchi_with_unreadable_password());

    enter_pin(&mut engine, PASSWORD);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);
}
