// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Revoking one of my devices must reach my other devices: otherwise they
//! keep syncing my data to the device I revoked (vauchi/private#917).

use vauchi_core::Vauchi;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::SyncItem;

/// My phone (this device), a tablet and a laptop, all on my identity.
fn phone_with_tablet_and_laptop() -> (Vauchi, [u8; 32], [u8; 32]) {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity("Alice").unwrap();
    let identity = phone.identity().unwrap();
    let seed = *identity.master_seed();
    let mut registry = identity.initial_device_registry();
    let tablet = DeviceInfo::derive(&seed, 1, "tablet".into(), 0);
    let laptop = DeviceInfo::derive(&seed, 2, "laptop".into(), 0);
    for device in [&tablet, &laptop] {
        registry
            .add_device(device.to_registered(&seed), identity.signing_keypair())
            .unwrap();
    }
    phone
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    (phone, *tablet.device_id(), *laptop.device_id())
}

fn registries_sent_to(phone: &Vauchi, device_id: &[u8; 32]) -> Vec<DeviceRegistry> {
    let registry = phone
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    DeviceSyncOrchestrator::load(
        phone.storage(),
        phone.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap()
    .pending_for_device(device_id)
    .iter()
    .filter_map(|item| match item {
        SyncItem::DeviceRegistryChanged { registry_json, .. } => {
            Some(DeviceRegistry::from_json(registry_json).unwrap())
        }
        _ => None,
    })
    .collect()
}

// @scenario: device_management :: Unlink a device remotely
#[test]
fn revoking_my_laptop_tells_my_tablet() {
    let (phone, tablet, laptop) = phone_with_tablet_and_laptop();
    let laptop_index = phone
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap()
        .all_devices()
        .iter()
        .position(|d| d.device_id == laptop)
        .unwrap();

    phone.revoke_device(laptop_index).unwrap();

    let laptop_revoked: Vec<bool> = registries_sent_to(&phone, &tablet)
        .iter()
        .map(|registry| registry.find_device(&laptop).is_some_and(|d| d.revoked))
        .collect();
    assert_eq!(laptop_revoked, vec![true]);
}

fn revoked_in_my_registry(phone: &Vauchi, device_id: &[u8; 32]) -> bool {
    phone
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap()
        .find_device(device_id)
        .is_some_and(|d| d.revoked)
}

// @scenario: device_management :: Unlink current device
#[test]
fn removing_my_identity_from_this_phone_tells_my_tablet_and_laptop() {
    let (phone, tablet, laptop) = phone_with_tablet_and_laptop();
    let this_phone = *phone.identity().unwrap().device_id();

    phone.revoke_this_device().unwrap();

    let told: Vec<Vec<bool>> = [tablet, laptop]
        .iter()
        .map(|device| {
            registries_sent_to(&phone, device)
                .iter()
                .map(|registry| registry.find_device(&this_phone).is_some_and(|d| d.revoked))
                .collect()
        })
        .collect();
    assert_eq!(told, vec![vec![true], vec![true]]);
}

// @scenario: device_management :: Removing the only device offers identity deletion instead
#[test]
fn the_last_device_of_an_identity_cannot_revoke_itself() {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity("Alice").unwrap();
    let this_phone = *phone.identity().unwrap().device_id();
    let registry = phone.identity().unwrap().initial_device_registry();
    phone
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();

    let refused = phone.revoke_this_device().is_err();

    assert_eq!(
        (refused, revoked_in_my_registry(&phone, &this_phone)),
        (true, false)
    );
}

// @scenario: device_management :: Removing a device in duress mode is the emergency wipe
#[test]
fn in_duress_mode_this_device_never_revokes_itself_in_the_real_registry() {
    let (mut phone, _tablet, _laptop) = phone_with_tablet_and_laptop();
    let this_phone = *phone.identity().unwrap().device_id();
    phone.setup_app_password("app-pin-1234").unwrap();
    phone
        .setup_duress_pin_with_biometric_unlock("135790")
        .unwrap();
    assert_eq!(
        phone.authenticate("135790").unwrap(),
        vauchi_core::AuthMode::Duress
    );

    let refused = phone.revoke_this_device().is_err();
    // The real registry is out of the duress foreground's reach (ADR-080).
    phone.authenticate("app-pin-1234").unwrap();

    assert_eq!(
        (refused, revoked_in_my_registry(&phone, &this_phone)),
        (true, false)
    );
}

const ANOTHER_IDENTITYS_REGISTRY: &str = "the saved device registry belongs to another identity";

/// My phone, holding a registry that names it and a laptop but that
/// another identity signed: what an identity this phone held before left.
fn phone_over_another_identitys_registry() -> Vauchi {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity("Alice").unwrap();
    let identity = phone.identity().unwrap();
    let seed = *identity.master_seed();
    let mut registry = identity.initial_device_registry();
    let laptop = DeviceInfo::derive(&seed, 1, "laptop".into(), 0);
    registry
        .add_device(laptop.to_registered(&seed), identity.signing_keypair())
        .unwrap();
    let other = vauchi_core::identity::Identity::create("Mallory", 1_700_000_000);
    registry.lift_version_to(registry.version() + 1, other.signing_keypair());
    phone
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    phone
}

fn saved_registry_verifies_as_mine(phone: &Vauchi) -> bool {
    let registry = phone
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    registry.verify(&phone.identity().unwrap().signing_keypair().public_key())
}

// @internal
#[test]
fn revoking_a_device_does_not_re_sign_another_identitys_registry() {
    let phone = phone_over_another_identitys_registry();

    let error = phone
        .revoke_device(1)
        .expect_err("another identity's registry is not revoked in and re-signed");

    assert!(
        error.to_string().contains(ANOTHER_IDENTITYS_REGISTRY),
        "got: {error}"
    );
    assert!(!saved_registry_verifies_as_mine(&phone), "left as it was");
}

// @internal
#[test]
fn removing_this_device_does_not_re_sign_another_identitys_registry() {
    let phone = phone_over_another_identitys_registry();

    let error = phone
        .revoke_this_device()
        .expect_err("another identity's registry is not revoked in and re-signed");

    assert!(
        error.to_string().contains(ANOTHER_IDENTITYS_REGISTRY),
        "got: {error}"
    );
    assert!(!saved_registry_verifies_as_mine(&phone), "left as it was");
}
