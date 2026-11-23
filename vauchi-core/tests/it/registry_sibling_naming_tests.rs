// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device naming its own key in this identity's registry tells its
//! siblings, and a sibling's registry naming another key at the same
//! version is merged and sent on (private#970).

use crate::common::device_sync::{journal_for_tablet, link_tablet};
use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::Vauchi;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::DeviceRegistry;
use vauchi_core::sync::SyncItem;

fn registries_journaled(journal: &[SyncItem]) -> Vec<DeviceRegistry> {
    journal
        .iter()
        .filter_map(|item| match item {
            SyncItem::DeviceRegistryChanged { registry_json, .. } => {
                Some(DeviceRegistry::from_json(registry_json).unwrap())
            }
            _ => None,
        })
        .collect()
}

fn phone_key(phone: &Vauchi) -> [u8; 32] {
    *phone
        .storage()
        .device()
        .load_or_create_device_signing_key(phone.identity().unwrap())
        .unwrap()
        .public_key()
        .as_bytes()
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn naming_this_devices_key_is_journaled_for_its_siblings() {
    let phone = create_vauchi_with_identity("Alice");
    let (registry, tablet) = link_tablet(&phone);

    phone
        .own_registry_broadcast(phone.identity().unwrap())
        .unwrap();

    let journaled = registries_journaled(&journal_for_tablet(&phone, registry, &tablet));
    let identity = phone.identity().unwrap();
    assert!(
        journaled.iter().any(|r| r
            .find_device(identity.device_id())
            .and_then(|d| d.signing_public_key)
            == Some(phone_key(&phone))),
        "no registry naming the phone's key was journaled for the tablet: {journaled:?}"
    );
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_siblings_registry_naming_its_key_at_this_version_is_merged_and_sent_on() {
    let phone = create_vauchi_with_identity("Alice");
    let (registry, tablet) = link_tablet(&phone);
    let identity = phone.identity().unwrap();
    let named_here = phone
        .storage()
        .device()
        .own_device_registry(identity)
        .unwrap();
    let tablet_key = SigningKeyPair::generate();
    let mut named_there = registry.clone();
    named_there
        .name_device_key(
            &tablet,
            *tablet_key.public_key().as_bytes(),
            identity.signing_keypair(),
        )
        .unwrap();
    assert_eq!(named_there.version(), named_here.version());

    phone
        .apply_sync_items(vec![SyncItem::DeviceRegistryChanged {
            registry_json: named_there.to_json(),
            version: named_there.version(),
        }])
        .unwrap();

    let saved = phone
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    assert!(saved.version() > named_here.version());
    let key_of = |id: &[u8; 32]| saved.find_device(id).and_then(|d| d.signing_public_key);
    assert_eq!(key_of(identity.device_id()), Some(phone_key(&phone)));
    assert_eq!(key_of(&tablet), Some(*tablet_key.public_key().as_bytes()));
    let journaled = registries_journaled(&journal_for_tablet(&phone, registry, &tablet));
    assert!(
        journaled.iter().any(|r| r.version() == saved.version()),
        "the merge was not journaled for the tablet"
    );
}
