// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! No two of a backup's passwords may match: a backup or decoy backup
//! password that unlocks the install, or two equal backup passwords, would
//! let one guess open two layers (ADR-080 point 7, amended 2026-11-09;
//! vauchi/private#1117).

use vauchi_core::contact_card::ContactCard;
use vauchi_core::{AuthMode, Vauchi};

const APP_PASSWORD: &str = "Correct-Horse-App-Pass-71";
const DURESS_PIN: &str = "112233";
const BACKUP_PASSWORD: &str = "a-distinct-backup-passphrase-93";
const DECOY_PASSWORD: &str = "yet-another-decoy-passphrase-57";

fn alice() -> Vauchi {
    let mut alice = Vauchi::in_memory().expect("install");
    alice.create_identity("Alice").expect("identity");
    alice
        .setup_app_password(APP_PASSWORD)
        .expect("app password");
    alice
        .setup_duress_password(DURESS_PIN)
        .expect("duress password");
    alice
        .add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
        .expect("decoy");
    assert_eq!(
        alice.authenticate(APP_PASSWORD).expect("unlock"),
        AuthMode::Normal
    );
    alice
}

fn reuse_error(vauchi: &Vauchi) -> String {
    vauchi
        .export_full_backup(APP_PASSWORD)
        .expect_err("the app password is refused as a backup password")
        .to_string()
}

// @scenario: backup_format_versioning :: Full backup refuses a weak password
#[test]
fn distinct_strong_passwords_export() {
    let alice = alice();

    assert!(alice.export_full_backup(BACKUP_PASSWORD).is_ok());
    assert!(
        alice
            .export_full_backup_with_decoy(BACKUP_PASSWORD, DECOY_PASSWORD)
            .is_ok()
    );
}

// @scenario: backup_format_versioning :: Full backup refuses a weak password
#[test]
fn a_backup_password_that_unlocks_the_install_is_refused() {
    let alice = alice();
    let expected = reuse_error(&alice);

    for password in [APP_PASSWORD, DURESS_PIN] {
        let full = alice.export_full_backup(password).expect_err("refused");
        let with_decoy = alice
            .export_full_backup_with_decoy(password, DECOY_PASSWORD)
            .expect_err("refused");
        assert_eq!(full.to_string(), expected);
        assert_eq!(with_decoy.to_string(), expected);
    }
}

// @scenario: backup_format_versioning :: Full backup refuses a weak password
#[test]
fn a_decoy_password_equal_to_another_password_is_refused() {
    let alice = alice();
    let expected = reuse_error(&alice);

    for decoy in [BACKUP_PASSWORD, APP_PASSWORD, DURESS_PIN] {
        let error = alice
            .export_full_backup_with_decoy(BACKUP_PASSWORD, decoy)
            .expect_err("refused");
        assert_eq!(error.to_string(), expected);
    }
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn under_duress_the_duress_pin_is_refused_as_the_app_password_is_on_the_real_profile() {
    let mut alice = alice();
    let real_refusal = reuse_error(&alice);
    assert_eq!(
        alice.authenticate(DURESS_PIN).expect("unlock"),
        AuthMode::Duress
    );

    let duress_refusal = alice
        .export_full_backup(DURESS_PIN)
        .expect_err("refused under duress");

    assert_eq!(duress_refusal.to_string(), real_refusal);
}
