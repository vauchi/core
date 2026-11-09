// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A full backup exported from the real profile with a duress PIN set also
//! takes a decoy backup password, which opens the decoy payload: the decoy
//! content under the mask identity, locked with the duress credential and
//! with no duress of its own (ADR-080 point 7, amended 2026-11-09;
//! vauchi/private#1117).

use vauchi_core::contact_card::ContactCard;
use vauchi_core::{AuthMode, Vauchi};

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-real";
const DECOY_PASSWORD: &str = "a-quite-different-decoy-passphrase";
const APP_PASSWORD: &str = "normal-pin-1234";
const DURESS_PIN: &str = "112233";

fn signing_key(vauchi: &Vauchi) -> [u8; 32] {
    *vauchi.identity().expect("identity").signing_public_key()
}

fn contact_names(vauchi: &Vauchi) -> Vec<String> {
    let mut names: Vec<String> = vauchi
        .list_contacts()
        .expect("contacts")
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    names.sort();
    names
}

struct Alice {
    vauchi: Vauchi,
    mask_key: [u8; 32],
}

fn alice(with_duress_pin: bool) -> Alice {
    let mut vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    vauchi
        .setup_app_password(APP_PASSWORD)
        .expect("app password");
    let mut mask_key = [0; 32];
    if with_duress_pin {
        vauchi
            .setup_duress_password(DURESS_PIN)
            .expect("duress PIN");
        vauchi
            .add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
            .expect("decoy");
        assert_eq!(
            vauchi.authenticate(DURESS_PIN).expect("unlock"),
            AuthMode::Duress
        );
        mask_key = signing_key(&vauchi);
    }
    assert_eq!(
        vauchi.authenticate(APP_PASSWORD).expect("unlock"),
        AuthMode::Normal
    );
    Alice { vauchi, mask_key }
}

fn restored(backup: &str, password: &str) -> Vauchi {
    let mut fresh = Vauchi::in_memory().expect("fresh install");
    fresh
        .import_full_backup(backup, password)
        .expect("the backup restores");
    fresh
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn the_decoy_password_restores_the_decoys_under_the_mask_identity() {
    let alice = alice(true);
    let backup = alice
        .vauchi
        .export_full_backup_with_decoy(BACKUP_PASSWORD, DECOY_PASSWORD)
        .expect("export");

    let decoy = restored(&backup, DECOY_PASSWORD);

    assert_eq!(signing_key(&decoy), alice.mask_key);
    assert_eq!(contact_names(&decoy), vec!["Decoy Dana".to_string()]);
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_restored_decoy_backup_locks_with_the_duress_pin_and_has_no_duress() {
    let alice = alice(true);
    let backup = alice
        .vauchi
        .export_full_backup_with_decoy(BACKUP_PASSWORD, DECOY_PASSWORD)
        .expect("export");
    let mut decoy = restored(&backup, DECOY_PASSWORD);

    assert!(decoy.is_password_enabled().expect("password state"));
    assert!(!decoy.is_duress_enabled().expect("duress state"));
    assert!(decoy.authenticate(APP_PASSWORD).is_err());
    assert_eq!(
        decoy.authenticate(DURESS_PIN).expect("unlock"),
        AuthMode::Normal
    );
    assert_eq!(signing_key(&decoy), alice.mask_key);
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn the_backup_password_of_the_same_file_restores_the_real_profile() {
    let alice = alice(true);
    let real_key = signing_key(&alice.vauchi);
    let backup = alice
        .vauchi
        .export_full_backup_with_decoy(BACKUP_PASSWORD, DECOY_PASSWORD)
        .expect("export");

    let mut real = restored(&backup, BACKUP_PASSWORD);

    assert_eq!(signing_key(&real), real_key);
    assert_eq!(
        real.authenticate(APP_PASSWORD).expect("unlock"),
        AuthMode::Normal
    );
    assert_eq!(
        real.authenticate(DURESS_PIN).expect("unlock"),
        AuthMode::Duress
    );
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_decoy_password_is_refused_without_a_duress_pin_and_under_duress_alike() {
    let without_pin = alice(false);
    let mut under_duress = alice(true).vauchi;
    under_duress.authenticate(DURESS_PIN).expect("unlock");

    let without_pin_error = without_pin
        .vauchi
        .export_full_backup_with_decoy(BACKUP_PASSWORD, DECOY_PASSWORD)
        .expect_err("no decoy to export");
    let under_duress_error = under_duress
        .export_full_backup_with_decoy(BACKUP_PASSWORD, DECOY_PASSWORD)
        .expect_err("no decoy to export under duress");

    assert_eq!(
        under_duress_error.to_string(),
        without_pin_error.to_string()
    );
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_backup_made_under_duress_restores_as_a_decoy_backup_does() {
    let mut alice = alice(true);
    alice.vauchi.authenticate(DURESS_PIN).expect("unlock");
    let backup = alice
        .vauchi
        .export_full_backup(BACKUP_PASSWORD)
        .expect("a duress export");

    let mut restored = restored(&backup, BACKUP_PASSWORD);

    assert!(restored.is_password_enabled().expect("password state"));
    assert!(!restored.is_duress_enabled().expect("duress state"));
    assert!(restored.authenticate(APP_PASSWORD).is_err());
    assert_eq!(
        restored.authenticate(DURESS_PIN).expect("unlock"),
        AuthMode::Normal
    );
    assert_eq!(signing_key(&restored), alice.mask_key);
    assert_eq!(contact_names(&restored), vec!["Decoy Dana".to_string()]);
}
