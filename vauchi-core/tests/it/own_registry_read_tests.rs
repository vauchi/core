// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A saved registry another identity signed counts as no registry wherever
//! it is only read (ADR-079, private#970): the device list, setup progress
//! and loss safeguards do not count its devices, and a sibling's registry
//! replaces it whatever its version.

use vauchi_core::Vauchi;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, Identity};
use vauchi_core::sync::SyncItem;

/// Above any version Alice's own registry reaches in these tests, so a
/// version comparison against it would refuse hers.
const MALLORYS_VERSION: u64 = 1_000_000;

/// Alice's registry: her phone and a laptop, signed by her.
fn alices_two_devices(alice: &Identity) -> DeviceRegistry {
    let seed = *alice.master_seed();
    let mut registry = alice.initial_device_registry();
    let laptop = DeviceInfo::derive(&seed, 1, "laptop".into(), 0);
    registry
        .add_device(laptop.to_registered(&seed), alice.signing_keypair())
        .unwrap();
    registry
}

/// Alice's phone over her two devices' registry, last signed by Mallory.
fn phone_over_mallorys_registry() -> Vauchi {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity("Alice").unwrap();
    let mut registry = alices_two_devices(phone.identity().unwrap());
    let mallory = Identity::create("Mallory", 1_700_000_000);
    registry.lift_version_to(MALLORYS_VERSION, mallory.signing_keypair());
    phone
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    phone
}

fn laptop_prefix(phone: &Vauchi) -> String {
    let seed = *phone.identity().unwrap().master_seed();
    let laptop = DeviceInfo::derive(&seed, 1, "laptop".into(), 0);
    hex::encode(&laptop.device_id()[..8])
}

fn saved_registry_is_alices_two_devices(phone: &Vauchi) -> bool {
    let registry = phone
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    registry.verify(&phone.identity().unwrap().signing_keypair().public_key())
        && registry.device_count() == 2
}

// @internal
#[test]
fn the_device_list_shows_only_this_device_over_another_identitys_registry() {
    let phone = phone_over_mallorys_registry();

    let devices = phone.list_devices().unwrap();

    assert_eq!(devices.len(), 1, "{devices:?}");
    assert!(devices[0].is_current);
}

// @internal
#[test]
fn setup_progress_counts_no_linked_device_over_another_identitys_registry() {
    let phone = phone_over_mallorys_registry();

    assert!(!phone.get_setup_progress().unwrap().device_linked);
}

// @internal
#[test]
fn loss_safeguards_count_no_linked_device_over_another_identitys_registry() {
    let phone = phone_over_mallorys_registry();

    assert!(!phone.loss_safeguards().unwrap().linked_device);
}

// @internal
#[test]
fn no_device_is_found_by_prefix_in_another_identitys_registry() {
    let phone = phone_over_mallorys_registry();
    let prefix = laptop_prefix(&phone);

    let found = phone.find_device_by_prefix(&prefix).unwrap();
    assert!(found.is_none(), "found {:?}", found.map(|d| d.device_name));
}

// @internal
#[test]
fn a_siblings_registry_replaces_another_identitys_whatever_its_version() {
    let phone = phone_over_mallorys_registry();
    let alices = alices_two_devices(phone.identity().unwrap());
    assert!(alices.version() < MALLORYS_VERSION);

    phone
        .apply_sync_items(vec![SyncItem::DeviceRegistryChanged {
            registry_json: alices.to_json(),
            version: alices.version(),
        }])
        .unwrap();

    assert!(saved_registry_is_alices_two_devices(&phone));
}

// @internal
#[test]
fn a_registry_change_is_persisted_over_another_identitys_whatever_its_version() {
    let phone = phone_over_mallorys_registry();
    let identity = phone.identity().unwrap();
    let alices = alices_two_devices(identity);

    DeviceSyncOrchestrator::persist_device_registry_change(
        phone.storage(),
        identity,
        &alices,
        1_700_000_000,
    )
    .unwrap();

    assert!(saved_registry_is_alices_two_devices(&phone));
}
