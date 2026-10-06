// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress mode reads as a single-device app and cannot link or revoke
//! devices (#469, ADR-032; owner decision 2026-10-02): linking would hand
//! the identity to the coercer's phone, revoking would change the real
//! registry, and the device list would name the owner's other devices.

use super::common::helpers::create_vauchi_with_identity;
use vauchi_core::identity::DeviceInfo;
use vauchi_core::{AuthMode, Vauchi};

const APP_PIN: &str = "app-pin-1234";
const DURESS_PIN: &str = "135790";
const OTHER_SEED: [u8; 32] = [9u8; 32];

/// The owner's identity with a second, real linked device, unlocked with
/// the duress PIN.
fn in_duress_mode() -> Vauchi {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password(APP_PIN).unwrap();
    let mut registry = wb.identity().unwrap().initial_device_registry();
    registry
        .add_device_unsigned(
            DeviceInfo::derive(&OTHER_SEED, 1, "RealLaptop".into(), 0).to_registered(&OTHER_SEED),
        )
        .unwrap();
    wb.storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    wb
}

fn names(wb: &Vauchi) -> Vec<String> {
    wb.list_devices()
        .unwrap()
        .into_iter()
        .map(|d| d.device_name)
        .collect()
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @internal
#[test]
fn duress_mode_lists_only_this_device() {
    let mut wb = in_duress_mode();
    let devices = wb.list_devices().unwrap();
    assert_eq!(devices.len(), 1, "{:?}", names(&wb));
    assert!(devices[0].is_current);
    assert!(!names(&wb).contains(&"RealLaptop".to_string()));

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    assert!(names(&wb).contains(&"RealLaptop".to_string()));
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @internal
#[test]
fn duress_mode_cannot_create_a_device_link() {
    let mut wb = in_duress_mode();
    assert!(wb.generate_device_link().is_err());
    assert!(wb.device_link_initiator().is_err());

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    assert!(wb.generate_device_link().is_ok());
    assert!(wb.device_link_initiator().is_ok());
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @internal
#[test]
fn duress_mode_cannot_revoke_a_real_device() {
    let mut wb = in_duress_mode();
    assert!(wb.revoke_device(1).is_err());

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    let laptop = wb
        .list_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.device_name == "RealLaptop")
        .expect("the real laptop is still registered");
    assert!(laptop.is_active, "a duress session must not revoke it");
}

// Linking asks for the PIN again (owner decision 2026-10-02). It must accept
// the PIN that opened this session, or a duress session would be told its
// own PIN is wrong; and it must never switch the session's mode.
// @scenario: duress_mode :: Duress mode looks identical to normal mode
// @internal
#[test]
fn confirming_the_pin_accepts_only_the_pin_that_opened_the_session() {
    let mut wb = in_duress_mode();
    assert!(wb.confirm_session_password(DURESS_PIN).unwrap());
    assert!(!wb.confirm_session_password(APP_PIN).unwrap());
    assert!(!wb.confirm_session_password("wrong").unwrap());
    assert_eq!(wb.auth_mode(), AuthMode::Duress);

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    assert!(wb.confirm_session_password(APP_PIN).unwrap());
    assert!(!wb.confirm_session_password(DURESS_PIN).unwrap());
    assert!(!wb.confirm_session_password("wrong").unwrap());
    assert_eq!(wb.auth_mode(), AuthMode::Normal);
}

/// A linked tablet (device index 1) unlocked with the duress PIN.
fn tablet_in_duress_mode() -> Vauchi {
    let seed = [4u8; 32];
    let mut wb = Vauchi::in_memory().unwrap();
    wb.set_identity(vauchi_core::Identity::from_device_link(
        seed,
        "Alice".into(),
        1,
        "Tablet".into(),
        0,
    ))
    .unwrap();
    wb.setup_app_password(APP_PIN).unwrap();
    let signing = vauchi_core::crypto::SigningKeyPair::from_seed(&seed);
    let mut registry = vauchi_core::identity::DeviceRegistry::new(
        DeviceInfo::derive(&seed, 0, "Phone".into(), 0).to_registered(&seed),
        &signing,
    );
    registry
        .add_device_unsigned(DeviceInfo::derive(&seed, 1, "Tablet".into(), 0).to_registered(&seed))
        .unwrap();
    wb.storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    wb
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @internal
#[test]
fn duress_mode_shows_a_linked_device_as_the_only_device_at_index_zero() {
    let mut wb = tablet_in_duress_mode();

    let devices = wb.list_devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(
        devices[0].device_index, 0,
        "the real index would reveal other devices"
    );

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    let current = wb
        .list_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.is_current)
        .unwrap();
    assert_eq!(current.device_index, 1);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @internal
#[test]
fn duress_mode_answers_revocation_like_a_single_device_app() {
    let wb = in_duress_mode();

    let current = wb.revoke_device(0).unwrap_err().to_string();
    let other = wb.revoke_device(5).unwrap_err().to_string();

    assert!(
        current.contains("Cannot revoke the current device"),
        "{current}"
    );
    assert!(other.contains("Invalid device index: 5"), "{other}");
}

// @internal
#[test]
fn outside_duress_only_another_existing_device_can_be_revoked() {
    let mut wb = in_duress_mode();
    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);

    let out_of_range = wb.revoke_device(2).unwrap_err().to_string();
    let current = wb.revoke_device(0).unwrap_err().to_string();
    assert!(
        out_of_range.contains("Invalid device index: 2"),
        "{out_of_range}"
    );
    assert!(
        current.contains("Cannot revoke the current device"),
        "{current}"
    );

    wb.revoke_device(1).unwrap();
    let laptop = wb
        .list_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.device_name == "RealLaptop")
        .unwrap();
    assert!(!laptop.is_active);
}
