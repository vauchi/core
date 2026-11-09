// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A duress unlock puts the mask engine in the foreground: its identity is
//! the mask seed's, and it shows the real public ID, fingerprint and name
//! from the mask's public values. An unlock that is not duress brings the
//! real engine back (ADR-080 point 4, amended 2026-11-08;
//! vauchi/private#1117).

use vauchi_core::{AuthMode, Vauchi};

const APP_PASSWORD: &str = "my-pin-1234";
const DURESS_PIN: &str = "654321";

struct Real {
    vauchi: Vauchi,
    public_id: String,
    fingerprint: String,
    signing_key: [u8; 32],
}

fn alice_with_a_duress_pin() -> Real {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password(APP_PASSWORD).unwrap();
    vauchi.setup_duress_password(DURESS_PIN).unwrap();
    Real {
        public_id: vauchi.public_id().unwrap(),
        fingerprint: vauchi.own_fingerprint().unwrap(),
        signing_key: *vauchi.identity().unwrap().signing_public_key(),
        vauchi,
    }
}

// @scenario: duress_mode :: Real keys never loaded in duress mode
#[test]
fn under_duress_the_identity_is_the_mask_seed_s_and_shows_the_real_public_values() {
    let mut real = alice_with_a_duress_pin();

    assert_eq!(
        real.vauchi.authenticate(DURESS_PIN).unwrap(),
        AuthMode::Duress
    );

    let identity = real.vauchi.identity().unwrap();
    assert_ne!(identity.signing_public_key(), &real.signing_key);
    assert_eq!(identity.display_name(), "Alice");
    assert_eq!(real.vauchi.public_id().unwrap(), real.public_id);
    assert_eq!(real.vauchi.own_fingerprint().unwrap(), real.fingerprint);
}

// @scenario: duress_mode :: Real keys never loaded in duress mode
#[test]
fn the_mask_seed_s_identity_is_the_same_in_every_duress_session() {
    let mut real = alice_with_a_duress_pin();
    real.vauchi.authenticate(DURESS_PIN).unwrap();
    let first = *real.vauchi.identity().unwrap().signing_public_key();

    real.vauchi.authenticate(APP_PASSWORD).unwrap();
    real.vauchi.authenticate(DURESS_PIN).unwrap();

    assert_eq!(real.vauchi.identity().unwrap().signing_public_key(), &first);
}

// @scenario: duress_mode :: Normal credential shows real contacts
#[test]
fn an_unlock_with_the_app_password_after_duress_brings_the_real_identity_back() {
    let mut real = alice_with_a_duress_pin();
    real.vauchi.authenticate(DURESS_PIN).unwrap();

    assert_eq!(
        real.vauchi.authenticate(APP_PASSWORD).unwrap(),
        AuthMode::Normal
    );

    assert_eq!(
        real.vauchi.identity().unwrap().signing_public_key(),
        &real.signing_key
    );
    assert_eq!(real.vauchi.public_id().unwrap(), real.public_id);
}

// @scenario: duress_mode :: What a duress session makes survives a lock
#[test]
fn a_rename_under_duress_is_there_after_the_real_profile_comes_back_and_duress_returns() {
    let mut real = alice_with_a_duress_pin();
    real.vauchi.authenticate(DURESS_PIN).unwrap();
    real.vauchi.update_display_name("Dana").unwrap();

    real.vauchi.authenticate(APP_PASSWORD).unwrap();
    let real_name = real
        .vauchi
        .own_card()
        .unwrap()
        .unwrap()
        .display_name()
        .to_string();
    real.vauchi.authenticate(DURESS_PIN).unwrap();
    let duress_name = real
        .vauchi
        .own_card()
        .unwrap()
        .unwrap()
        .display_name()
        .to_string();

    assert_eq!(
        (real_name.as_str(), duress_name.as_str()),
        ("Alice", "Dana")
    );
}
