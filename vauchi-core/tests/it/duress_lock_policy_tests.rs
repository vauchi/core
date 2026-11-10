// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress lock policy (ADR-032, amended 2026-11-10; vauchi/private#1117
//! task 10). A 6-digit duress PIN alone falls to an offline guess in
//! minutes, so a duress credential is set up under one of two branches:
//! biometric unlock in front of a 6-digit PIN, or a real password, under
//! the backup password's strength rule, for both the app password and the
//! duress credential. The password branch takes the current app password,
//! since Core keeps only its hash.

use crate::common;
use common::helpers::create_vauchi_with_identity;
use vauchi_core::api::{DuressUnlock, WeakCredential};
use vauchi_core::{AuthMode, Vauchi, VauchiError};

const STRONG_APP_PASSWORD: &str = "correct-horse-battery-staple-app";
const STRONG_DURESS_PASSWORD: &str = "plum-violin-orbit-meadow-duress";
const DURESS_PIN: &str = "654321";

/// Passwords the backup rule refuses: too short, or long but guessable.
fn weak_passwords() -> Vec<(&'static str, &'static str)> {
    vec![
        ("", "empty"),
        ("123456", "six digits"),
        ("1234567", "seven characters"),
        ("password", "a dictionary word"),
        ("password1234", "a dictionary word and a sequence"),
        ("aaaaaaaaaaaa", "one repeated letter"),
    ]
}

fn install_with_app_password(app_password: &str) -> Vauchi {
    let mut vauchi = create_vauchi_with_identity("Alice");
    vauchi
        .setup_app_password(app_password)
        .expect("the app password is set first");
    vauchi
}

fn duress_enabled(vauchi: &Vauchi) -> bool {
    vauchi.is_duress_enabled().expect("duress state loads")
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn an_install_without_duress_has_no_duress_branch() {
    let vauchi = install_with_app_password(STRONG_APP_PASSWORD);

    assert_eq!(vauchi.duress_unlock().expect("branch loads"), None);
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn a_pin_set_for_biometric_unlock_records_the_biometric_branch() {
    let mut vauchi = install_with_app_password("2468");

    vauchi
        .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .expect("six digits are the biometric branch's credential");

    assert_eq!(
        vauchi.duress_unlock().expect("branch loads"),
        Some(DuressUnlock::Biometric)
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn two_real_passwords_set_up_duress_under_the_password_branch() {
    let mut vauchi = install_with_app_password(STRONG_APP_PASSWORD);

    vauchi
        .setup_duress_password_with_app_password(STRONG_APP_PASSWORD, STRONG_DURESS_PASSWORD)
        .expect("both credentials pass the backup rule");

    assert!(duress_enabled(&vauchi));
    assert_eq!(
        vauchi.duress_unlock().expect("branch loads"),
        Some(DuressUnlock::Passwords)
    );
    assert_eq!(
        vauchi.authenticate(STRONG_DURESS_PASSWORD).expect("unlock"),
        AuthMode::Duress
    );
    assert_eq!(
        vauchi.authenticate(STRONG_APP_PASSWORD).expect("unlock"),
        AuthMode::Normal
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn the_password_branch_refuses_a_weak_app_password() {
    for (weak, why) in weak_passwords() {
        if weak.is_empty() {
            continue;
        }
        let mut vauchi = install_with_app_password(weak);

        let result = vauchi.setup_duress_password_with_app_password(weak, STRONG_DURESS_PASSWORD);

        assert!(
            matches!(
                result,
                Err(VauchiError::WeakCredential(WeakCredential::AppPassword))
            ),
            "app password {weak:?} ({why}) must be named as the weak one, got {result:?}"
        );
        assert!(!duress_enabled(&vauchi), "{why}: duress stays off");
    }
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn the_password_branch_refuses_a_weak_duress_password() {
    for (weak, why) in weak_passwords() {
        let mut vauchi = install_with_app_password(STRONG_APP_PASSWORD);

        let result = vauchi.setup_duress_password_with_app_password(STRONG_APP_PASSWORD, weak);

        assert!(
            matches!(
                result,
                Err(VauchiError::WeakCredential(WeakCredential::DuressPassword))
            ),
            "duress password {weak:?} ({why}) must be named as the weak one, got {result:?}"
        );
        assert!(!duress_enabled(&vauchi), "{why}: duress stays off");
    }
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn the_password_branch_refuses_a_wrong_app_password() {
    let mut vauchi = install_with_app_password(STRONG_APP_PASSWORD);

    let result = vauchi.setup_duress_password_with_app_password(
        "a-different-but-strong-passphrase",
        STRONG_DURESS_PASSWORD,
    );

    assert!(
        matches!(result, Err(VauchiError::InvalidState(_))),
        "a wrong app password is refused, got {result:?}"
    );
    assert!(!duress_enabled(&vauchi));
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn the_password_branch_refuses_the_app_password_as_the_duress_password() {
    let mut vauchi = install_with_app_password(STRONG_APP_PASSWORD);

    let result =
        vauchi.setup_duress_password_with_app_password(STRONG_APP_PASSWORD, STRONG_APP_PASSWORD);

    assert!(
        matches!(result, Err(VauchiError::InvalidState(_))),
        "one credential cannot open both profiles, got {result:?}"
    );
    assert!(!duress_enabled(&vauchi));
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn under_the_password_branch_the_app_password_cannot_become_weak() {
    let mut vauchi = install_with_app_password(STRONG_APP_PASSWORD);
    vauchi
        .setup_duress_password_with_app_password(STRONG_APP_PASSWORD, STRONG_DURESS_PASSWORD)
        .expect("duress set");

    let result = vauchi.change_app_password(STRONG_APP_PASSWORD, "123456");

    assert!(
        matches!(
            result,
            Err(VauchiError::WeakCredential(WeakCredential::AppPassword))
        ),
        "a weak new app password is refused, got {result:?}"
    );
    assert_eq!(
        vauchi.authenticate(STRONG_APP_PASSWORD).expect("unlock"),
        AuthMode::Normal,
        "the old app password still opens the real profile"
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn under_the_password_branch_the_app_password_changes_to_another_real_password() {
    let new_password = "lantern-quartz-harbor-thistle";
    let mut vauchi = install_with_app_password(STRONG_APP_PASSWORD);
    vauchi
        .setup_duress_password_with_app_password(STRONG_APP_PASSWORD, STRONG_DURESS_PASSWORD)
        .expect("duress set");

    vauchi
        .change_app_password(STRONG_APP_PASSWORD, new_password)
        .expect("a strong new app password is accepted");

    assert_eq!(
        vauchi.authenticate(new_password).expect("unlock"),
        AuthMode::Normal
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn under_the_biometric_branch_the_app_password_may_stay_a_pin() {
    let mut vauchi = install_with_app_password("2468");
    vauchi
        .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .expect("duress set");

    vauchi
        .change_app_password("2468", "1357")
        .expect("biometric unlock guards a PIN app password");

    assert_eq!(
        vauchi.authenticate("1357").expect("unlock"),
        AuthMode::Normal
    );
}
