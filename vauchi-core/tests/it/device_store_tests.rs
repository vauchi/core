// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the device-domain persistence view (`DeviceStore`).
//!
//! Part of problem `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).
//! `DeviceStore` owns `device_info` and `device_registry`; sync state is split
//! out into `SyncStore`.

use std::time::{SystemTime, UNIX_EPOCH};

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::DoubleRatchetState;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, RegistryBroadcast};
use vauchi_core::{DeviceStore, SigningKeyPair, Storage, SymmetricKey, X3DHKeyPair};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// A consumer scoped to the device domain receives only `&DeviceStore` — it is
// statically unable to reach contacts, sync, or any other table.
fn device_index_via_scoped_view(store: &DeviceStore<'_>) -> Option<u32> {
    store
        .load_device_info()
        .unwrap()
        .map(|(_, index, _, _)| index)
}

// @internal
#[test]
fn test_device_store_info_roundtrip() {
    let storage = test_storage();
    let device_id = [7u8; 32];

    assert_eq!(device_index_via_scoped_view(&storage.device()), None);
    assert!(!storage.device().has_device_info().unwrap());

    storage
        .device()
        .save_device_info(&device_id, 3, "Pixel", 1000)
        .unwrap();

    assert_eq!(device_index_via_scoped_view(&storage.device()), Some(3));
    assert!(storage.device().has_device_info().unwrap());
    // Visible through the legacy forwarding API — one connection.
    let (id, index, name, created) = storage.device().load_device_info().unwrap().unwrap();
    assert_eq!(id, device_id);
    assert_eq!(index, 3);
    assert_eq!(name, "Pixel");
    assert_eq!(created, 1000);
}

// @internal
#[test]
fn test_device_store_clear_info_wipes_row() {
    let storage = test_storage();
    storage
        .device()
        .save_device_info(&[1u8; 32], 0, "Phone", 1)
        .unwrap();

    storage.device().clear_device_info().unwrap();

    assert!(!storage.device().has_device_info().unwrap());
}

// @internal
#[test]
fn test_contact_device_registry_accepts_only_newer_identity_signed_broadcasts() {
    let storage = test_storage();
    let signing = SigningKeyPair::from_seed(&[7u8; 32]);
    let contact = Contact::from_exchange(
        *signing.public_key().as_bytes(),
        ContactCard::new("Grandson"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();

    let seed = [11u8; 32];
    let primary = DeviceInfo::derive(&seed, 0, "Phone".into(), 1);
    let mut registry = DeviceRegistry::new(primary.to_registered(&seed), &signing);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let first = RegistryBroadcast::new(&registry, &signing, now);
    storage
        .device()
        .save_contact_device_registry(contact.id(), &first, signing.public_key().as_bytes(), 60)
        .unwrap();
    assert_eq!(
        storage
            .device()
            .load_contact_active_devices(contact.id())
            .unwrap()
            .len(),
        1
    );

    let attacker = SigningKeyPair::from_seed(&[99u8; 32]);
    let forged = RegistryBroadcast::new(&registry, &attacker, now);
    assert!(
        storage
            .device()
            .save_contact_device_registry(
                contact.id(),
                &forged,
                signing.public_key().as_bytes(),
                60,
            )
            .is_err()
    );
    assert!(
        storage
            .device()
            .save_contact_device_registry(
                contact.id(),
                &first,
                signing.public_key().as_bytes(),
                60,
            )
            .is_err(),
        "replayed registry version must be rejected"
    );

    let second = DeviceInfo::derive(&seed, 1, "Laptop".into(), 1);
    registry
        .add_device(second.to_registered(&seed), &signing)
        .unwrap();
    let newer = RegistryBroadcast::new(&registry, &signing, now);
    storage
        .device()
        .save_contact_device_registry(contact.id(), &newer, signing.public_key().as_bytes(), 60)
        .unwrap();
    assert_eq!(
        storage
            .device()
            .load_contact_active_devices(contact.id())
            .unwrap()
            .len(),
        2
    );

    let revoked_id = *second.device_id();
    let ratchet = DoubleRatchetState::initialize_initiator(
        &SymmetricKey::generate(),
        *X3DHKeyPair::generate().public_key(),
    )
    .unwrap();
    storage
        .ratchets()
        .save_ratchet_state_for_device(contact.id(), &revoked_id, &ratchet, true)
        .unwrap();
    registry
        .revoke_device(&revoked_id, &signing, now + 1)
        .unwrap();
    let pruned = RegistryBroadcast::new(&registry, &signing, now + 1);
    storage
        .device()
        .save_contact_device_registry(contact.id(), &pruned, signing.public_key().as_bytes(), 60)
        .unwrap();
    assert!(
        storage
            .ratchets()
            .load_ratchet_state_for_device(contact.id(), &revoked_id)
            .unwrap()
            .is_none(),
        "revoked peer sessions must be pruned"
    );

    storage.delete_contact(contact.id()).unwrap();
    assert!(
        storage
            .device()
            .load_contact_device_registry(contact.id())
            .unwrap()
            .is_none()
    );
}

// @internal
#[test]
fn test_contact_device_registry_version_above_sqlite_range_is_rejected() {
    let storage = test_storage();
    let signing = SigningKeyPair::from_seed(&[17u8; 32]);
    let contact = Contact::from_exchange(
        *signing.public_key().as_bytes(),
        ContactCard::new("Version boundary"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();

    let seed = [23u8; 32];
    let primary = DeviceInfo::derive(&seed, 0, "Phone".into(), 1);
    let registry = DeviceRegistry::new(primary.to_registered(&seed), &signing);
    let mut registry_json: serde_json::Value = serde_json::from_str(&registry.to_json()).unwrap();
    registry_json["version"] = serde_json::json!(u64::MAX);
    let oversized = DeviceRegistry::from_json(&registry_json.to_string()).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let broadcast = RegistryBroadcast::new(&oversized, &signing, now);

    let error = storage
        .device()
        .save_contact_device_registry(
            contact.id(),
            &broadcast,
            signing.public_key().as_bytes(),
            60,
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Invalid data: contact device registry version exceeds storage range"
    );
    assert!(
        storage
            .device()
            .load_contact_device_registry(contact.id())
            .unwrap()
            .is_none()
    );
}
