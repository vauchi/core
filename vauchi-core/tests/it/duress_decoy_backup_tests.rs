// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A backup exported under duress is a working decoy backup: it restores
//! the decoy content under the mask's own seed, and it is refused for a
//! weak password exactly as a real export is (ADR-080 point 7,
//! vauchi/private#1117).

use vauchi_core::contact_card::ContactCard;
use vauchi_core::{AuthMode, Vauchi};

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-mask-9";
const WEAK_PASSWORD: &str = "abc";

fn alice_with_a_duress_pin() -> Vauchi {
    let mut alice = Vauchi::in_memory().expect("install");
    alice.create_identity("Alice").expect("identity");
    alice
        .setup_app_password("normal-pin-1234")
        .expect("app password");
    alice
        .setup_duress_pin_with_biometric_unlock("112233")
        .expect("duress PIN");
    alice
        .add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
        .expect("decoy");
    alice
}

fn under_duress() -> Vauchi {
    let mut alice = alice_with_a_duress_pin();
    assert_eq!(
        alice.authenticate("112233").expect("unlock"),
        AuthMode::Duress
    );
    alice
}

fn signing_key(vauchi: &Vauchi) -> [u8; 32] {
    *vauchi.identity().expect("identity").signing_public_key()
}

// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn a_full_backup_made_under_duress_restores_under_the_mask_seed() {
    let session = under_duress();
    let mask_key = signing_key(&session);

    let backup = session
        .export_full_backup(BACKUP_PASSWORD)
        .expect("a duress export works");
    let mut restored = Vauchi::in_memory().expect("fresh install");
    restored
        .import_full_backup(&backup, BACKUP_PASSWORD)
        .expect("a duress backup restores");

    assert_eq!(signing_key(&restored), mask_key);
}

// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn an_identity_backup_made_under_duress_restores_under_the_mask_seed() {
    let session = under_duress();
    let mask_key = signing_key(&session);

    let backup = session
        .export_backup(BACKUP_PASSWORD)
        .expect("a duress export works");
    let mut restored = Vauchi::in_memory().expect("fresh install");
    restored
        .import_backup(&backup, BACKUP_PASSWORD)
        .expect("a duress backup restores");

    assert_eq!(signing_key(&restored), mask_key);
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_duress_export_refuses_a_weak_password_as_a_real_export_does() {
    let mut real = alice_with_a_duress_pin();
    real.authenticate("normal-pin-1234").expect("unlock");
    let session = under_duress();

    for (real_refusal, duress_refusal) in [
        (
            real.export_full_backup(WEAK_PASSWORD),
            session.export_full_backup(WEAK_PASSWORD),
        ),
        (
            real.export_backup(WEAK_PASSWORD),
            session.export_backup(WEAK_PASSWORD),
        ),
    ] {
        let real_error = real_refusal
            .expect_err("a weak password is refused")
            .to_string();
        let duress_error = duress_refusal
            .expect_err("a weak password is refused under duress")
            .to_string();
        assert_eq!(duress_error, real_error);
    }
}
