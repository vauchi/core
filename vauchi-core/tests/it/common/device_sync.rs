// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-sync orchestrator arrange helpers.
//!
//! In-memory storage, a derived device, its signed single-device registry,
//! and an exchanged contact — the shared arrange phase of the
//! `DeviceSyncOrchestrator` test files.

use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::SyncItem;
use vauchi_core::{Storage, Vauchi};

pub fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

pub fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
    DeviceInfo::derive(master_seed, index, name.to_string(), 0)
}

pub fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
    let signing_key = SigningKeyPair::from_seed(master_seed);
    DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
}

pub fn create_test_contact(name: &str) -> Contact {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key, 0)
}

/// Registers a "tablet" sibling beside the current device, so mutations
/// on `vauchi` journal sync items for it.
pub fn link_tablet(vauchi: &Vauchi, seed: [u8; 32]) -> (DeviceRegistry, [u8; 32]) {
    let signing = SigningKeyPair::from_seed(&seed);
    let mut registry = DeviceRegistry::new(
        DeviceInfo::derive(&seed, 0, "phone".into(), 0).to_registered(&seed),
        &signing,
    );
    let tablet = DeviceInfo::derive(&seed, 1, "tablet".into(), 0);
    let tablet_id = *tablet.device_id();
    registry
        .add_device_unsigned(tablet.to_registered(&seed))
        .unwrap();
    vauchi
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    (registry, tablet_id)
}

/// The sync items journaled for the sibling registered by `link_tablet`.
pub fn journal_for_tablet(
    vauchi: &Vauchi,
    registry: DeviceRegistry,
    tablet_id: &[u8; 32],
) -> Vec<SyncItem> {
    DeviceSyncOrchestrator::load(
        vauchi.storage(),
        vauchi.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap()
    .pending_for_device(tablet_id)
    .to_vec()
}
