// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Under the biometric branch the lock screen's PIN counts only after a
//! biometric pass (ADR-032, amended 2026-11-10; vauchi/private#1117 task
//! 10 step 3). Every install with biometric unlock shows the same lock
//! screen, so a PIN typed before the pass is refused exactly as a wrong
//! one: the screen does not show that duress is set.

use vauchi_core::{AuthMode, Vauchi};

const APP_PIN: &str = "2468";
const DURESS_PIN: &str = "654321";
const STRONG_APP_PASSWORD: &str = "correct-horse-battery-staple-app";
const STRONG_DURESS_PASSWORD: &str = "plum-violin-orbit-meadow-duress";

const NO_PASS: bool = false;
const PASSED: bool = true;
const BIOMETRIC_DEVICE: bool = true;
const NO_BIOMETRICS: bool = false;

fn install_with(app_password: &str) -> Vauchi {
    let mut vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    vauchi
        .setup_app_password(app_password)
        .expect("app password");
    vauchi
}

fn biometric_branch() -> Vauchi {
    let mut vauchi = install_with(APP_PIN);
    vauchi
        .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .expect("duress set");
    vauchi
}

fn refusal(vauchi: &mut Vauchi, password: &str, biometric_passed: bool) -> String {
    let error = vauchi
        .authenticate_at_lock(password, biometric_passed, BIOMETRIC_DEVICE)
        .expect_err("refused");
    format!("{error:?}")
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn under_the_biometric_branch_a_pin_before_the_pass_is_refused_as_a_wrong_one() {
    let mut vauchi = biometric_branch();
    let wrong = refusal(&mut vauchi, "1111", PASSED);

    for pin in [APP_PIN, DURESS_PIN] {
        assert_eq!(
            refusal(&mut vauchi, pin, NO_PASS),
            wrong,
            "{pin} before the pass gets the wrong-PIN error"
        );
    }
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn under_the_biometric_branch_the_pin_after_the_pass_picks_the_profile() {
    let mut vauchi = biometric_branch();

    assert_eq!(
        vauchi
            .authenticate_at_lock(DURESS_PIN, PASSED, BIOMETRIC_DEVICE)
            .expect("unlock"),
        AuthMode::Duress
    );
    assert_eq!(
        vauchi
            .authenticate_at_lock(APP_PIN, PASSED, BIOMETRIC_DEVICE)
            .expect("unlock"),
        AuthMode::Normal
    );
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn after_a_duress_session_the_lock_still_waits_for_the_pass() {
    let mut vauchi = biometric_branch();
    vauchi
        .authenticate_at_lock(DURESS_PIN, PASSED, BIOMETRIC_DEVICE)
        .expect("duress unlock");
    let wrong = refusal(&mut vauchi, "1111", PASSED);

    assert_eq!(refusal(&mut vauchi, APP_PIN, NO_PASS), wrong);
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn without_duress_the_password_opens_the_app_without_a_pass() {
    let mut vauchi = install_with(APP_PIN);

    assert_eq!(
        vauchi
            .authenticate_at_lock(APP_PIN, NO_PASS, BIOMETRIC_DEVICE)
            .expect("unlock"),
        AuthMode::Normal
    );
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn the_password_branch_takes_the_password_without_a_pass() {
    let mut vauchi = install_with(STRONG_APP_PASSWORD);
    vauchi
        .setup_duress_password_with_app_password(STRONG_APP_PASSWORD, STRONG_DURESS_PASSWORD)
        .expect("duress set");

    assert_eq!(
        vauchi
            .authenticate_at_lock(STRONG_DURESS_PASSWORD, NO_PASS, BIOMETRIC_DEVICE)
            .expect("unlock"),
        AuthMode::Duress
    );
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn a_device_without_biometrics_is_never_locked_out_by_the_biometric_branch() {
    let mut vauchi = biometric_branch();

    assert_eq!(
        vauchi
            .authenticate_at_lock(APP_PIN, NO_PASS, NO_BIOMETRICS)
            .expect("unlock"),
        AuthMode::Normal
    );
}
