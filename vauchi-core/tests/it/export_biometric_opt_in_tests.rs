// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The opt-in biometric check before a backup export is mirrored to the
//! mask, so an export under duress shows the same step as one on the real
//! profile (ADR-080 point 7, amended 2026-11-09; vauchi/private#1117).

use vauchi_core::{AuthMode, Vauchi};

const APP_PASSWORD: &str = "normal-pin-1234";
const DURESS_PIN: &str = "112233";

fn alice() -> Vauchi {
    let mut alice = Vauchi::in_memory().expect("install");
    alice.create_identity("Alice").expect("identity");
    alice
        .setup_app_password(APP_PASSWORD)
        .expect("app password");
    alice.setup_duress_password(DURESS_PIN).expect("duress PIN");
    assert_eq!(
        alice.authenticate(APP_PASSWORD).expect("unlock"),
        AuthMode::Normal
    );
    alice
}

fn opted_in(vauchi: &Vauchi) -> bool {
    vauchi
        .confirms_exports_with_biometrics()
        .expect("the opt-in reads")
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn the_opt_in_is_off_until_chosen() {
    let alice = alice();

    assert!(!opted_in(&alice));
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn an_opt_in_on_the_real_profile_shows_under_duress() {
    let mut alice = alice();
    alice
        .set_confirm_exports_with_biometrics(true)
        .expect("opt in");

    alice.authenticate(DURESS_PIN).expect("unlock");

    assert!(opted_in(&alice));
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn opting_out_on_the_real_profile_shows_under_duress() {
    let mut alice = alice();
    alice
        .set_confirm_exports_with_biometrics(true)
        .expect("opt in");
    alice
        .set_confirm_exports_with_biometrics(false)
        .expect("opt out");

    alice.authenticate(DURESS_PIN).expect("unlock");

    assert!(!opted_in(&alice));
}

// @scenario: duress_mode :: Edits in duress mode are discarded
#[test]
fn a_change_under_duress_does_not_reach_the_real_profile() {
    let mut alice = alice();
    alice.authenticate(DURESS_PIN).expect("unlock");
    alice
        .set_confirm_exports_with_biometrics(true)
        .expect("opt in under duress");

    alice.authenticate(APP_PASSWORD).expect("unlock");

    assert!(!opted_in(&alice));
}
