// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A full backup keeps the branch duress was set up under (ADR-032,
//! amended 2026-11-10; vauchi/private#1117 task 10). Restored under the
//! wrong branch, a password-branch install would ask for biometrics it may
//! not have, and its app password could change to a weak one.

use vauchi_core::api::{DuressUnlock, WeakCredential};
use vauchi_core::{Vauchi, VauchiError};

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-branch";
const STRONG_APP_PASSWORD: &str = "lantern-quartz-harbor-thistle-app";
const STRONG_DURESS_PASSWORD: &str = "plum-violin-orbit-meadow-duress";

fn install(app_password: &str) -> Vauchi {
    let mut vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Alice").expect("identity");
    vauchi
        .setup_app_password(app_password)
        .expect("app password");
    vauchi
}

fn restored(backup: &str) -> Vauchi {
    let mut fresh = Vauchi::in_memory().expect("fresh install");
    fresh
        .import_full_backup(backup, BACKUP_PASSWORD)
        .expect("a real backup restores");
    fresh
}

fn exported_under_the_password_branch() -> String {
    let mut alice = install(STRONG_APP_PASSWORD);
    alice
        .setup_duress_password_with_app_password(STRONG_APP_PASSWORD, STRONG_DURESS_PASSWORD)
        .expect("duress set");
    alice.export_full_backup(BACKUP_PASSWORD).expect("export")
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_restore_keeps_the_password_branch() {
    let fresh = restored(&exported_under_the_password_branch());

    assert_eq!(
        fresh.duress_unlock().expect("branch loads"),
        Some(DuressUnlock::Passwords)
    );
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_restored_password_branch_keeps_the_app_password_real() {
    let mut fresh = restored(&exported_under_the_password_branch());

    let result = fresh.change_app_password(STRONG_APP_PASSWORD, "123456");

    assert!(
        matches!(
            result,
            Err(VauchiError::WeakCredential(WeakCredential::AppPassword))
        ),
        "the restored install still holds the app password to the rule, got {result:?}"
    );
}

// @scenario: duress_mode :: Backup includes duress profile
#[test]
fn a_restore_keeps_the_biometric_branch() {
    let mut alice = install("2468");
    alice
        .setup_duress_pin_with_biometric_unlock("654321")
        .expect("duress set");

    let fresh = restored(&alice.export_full_backup(BACKUP_PASSWORD).expect("export"));

    assert_eq!(
        fresh.duress_unlock().expect("branch loads"),
        Some(DuressUnlock::Biometric)
    );
}
