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

// @scenario: duress_mode :: The duress setup is invisible in duress mode
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

// @scenario: duress_mode :: The duress setup is invisible in duress mode
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

// @scenario: duress_mode :: The duress setup is invisible in duress mode
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
