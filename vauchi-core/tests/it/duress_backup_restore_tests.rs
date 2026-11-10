// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A normal full backup carries the duress section, so a restore keeps
//! duress set up: the app password and duress PIN unlock as before, and
//! the duress PIN opens the same decoys under the same mask identity
//! (ADR-080 point 7, amended 2026-11-09; vauchi/private#1117).

use vauchi_core::contact_card::ContactCard;
use vauchi_core::{AuthMode, Vauchi};

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-restore";
const APP_PASSWORD: &str = "normal-pin-1234";
const DURESS_PIN: &str = "112233";

fn signing_key(vauchi: &Vauchi) -> [u8; 32] {
    *vauchi.identity().expect("identity").signing_public_key()
}

fn decoy_names(vauchi: &Vauchi) -> Vec<String> {
    let mut names: Vec<String> = vauchi
        .list_contacts()
        .expect("contacts")
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    names.sort();
    names
}

struct Exported {
    backup: String,
    real_key: [u8; 32],
    mask_key: [u8; 32],
}

/// Alice sets a duress PIN and two decoys, notes the mask identity under
/// duress, and exports a full backup from the real profile.
fn exported_from_the_real_profile() -> Exported {
    let mut alice = Vauchi::in_memory().expect("install");
    alice.create_identity("Alice").expect("identity");
    alice
        .setup_app_password(APP_PASSWORD)
        .expect("app password");
    alice
        .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .expect("duress PIN");
    for (id, name) in [("decoy-dana", "Decoy Dana"), ("decoy-eli", "Decoy Eli")] {
        alice
            .add_decoy_contact(id, name, &ContactCard::new(name))
            .expect("decoy");
    }
    assert_eq!(
        alice.authenticate(DURESS_PIN).expect("unlock"),
        AuthMode::Duress
    );
    let mask_key = signing_key(&alice);
    assert_eq!(
        alice.authenticate(APP_PASSWORD).expect("unlock"),
        AuthMode::Normal
    );
    Exported {
        backup: alice.export_full_backup(BACKUP_PASSWORD).expect("export"),
        real_key: signing_key(&alice),
        mask_key,
    }
}

fn restored(backup: &str) -> Vauchi {
    let mut fresh = Vauchi::in_memory().expect("fresh install");
    fresh
        .import_full_backup(backup, BACKUP_PASSWORD)
        .expect("a real backup restores");
    fresh
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_restored_install_unlocks_with_the_old_app_password() {
    let exported = exported_from_the_real_profile();
    let mut fresh = restored(&exported.backup);

    assert!(fresh.is_password_enabled().expect("password state"));
    assert_eq!(
        fresh.authenticate(APP_PASSWORD).expect("unlock"),
        AuthMode::Normal
    );
    assert_eq!(signing_key(&fresh), exported.real_key);
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_restored_install_opens_the_old_decoys_under_the_old_mask_with_the_duress_pin() {
    let exported = exported_from_the_real_profile();
    let mut fresh = restored(&exported.backup);

    assert_eq!(
        fresh.authenticate(DURESS_PIN).expect("unlock"),
        AuthMode::Duress
    );
    assert_eq!(signing_key(&fresh), exported.mask_key);
    assert_eq!(
        decoy_names(&fresh),
        vec!["Decoy Dana".to_string(), "Decoy Eli".to_string()]
    );
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_restored_install_refuses_a_credential_it_never_had() {
    let exported = exported_from_the_real_profile();
    let mut fresh = restored(&exported.backup);

    assert!(fresh.authenticate("999999").is_err());
    assert!(fresh.authenticate("another-wrong-password").is_err());
}
