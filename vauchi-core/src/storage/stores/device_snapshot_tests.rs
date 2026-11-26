// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::card::ContactCard;
use crate::clock::FakeClock;
use crate::contact::Contact;
use std::time::{Duration, UNIX_EPOCH};

// @internal
#[test]
fn registry_acceptance_refuses_version_metadata_from_a_different_snapshot() {
    const NOW: u64 = 1_800_000_000;
    let owner = Identity::create("Alice", NOW);
    let device_key = SigningKeyPair::generate();
    let storage = Storage::in_memory(SymmetricKey::generate())
        .unwrap()
        .with_clock(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)).shared());
    let contact = Contact::from_exchange(
        *owner.signing_public_key(),
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        NOW,
    );
    storage.contacts().save_contact(&contact).unwrap();
    let mut registry = DeviceRegistry::new(
        owner
            .device_info()
            .to_registered(owner.master_seed())
            .with_signing_key(*device_key.public_key().as_bytes()),
        owner.signing_keypair(),
    );
    registry
        .set_notice_cohort(owner.device_id(), Some(7), owner.signing_keypair())
        .unwrap();
    let broadcast = |registry: &DeviceRegistry| {
        RegistryBroadcast::signed_by_device(
            registry,
            owner.signing_keypair(),
            *owner.device_id(),
            &device_key,
            NOW,
        )
    };
    storage
        .device()
        .save_contact_device_registry(
            contact.id(),
            &broadcast(&registry),
            owner.signing_public_key(),
            60,
        )
        .unwrap();
    let held_version = registry.version();
    storage
        .conn
        .execute(
            "UPDATE contact_device_registries SET version = version + 1 WHERE contact_id = ?1",
            [contact.id()],
        )
        .unwrap();
    registry.lift_version_to(held_version + 2, owner.signing_keypair());
    let result = storage.device().save_contact_device_registry(
        contact.id(),
        &broadcast(&registry),
        owner.signing_public_key(),
        60,
    );
    assert!(matches!(result, Err(StorageError::InvalidData(_))));
    storage
        .conn
        .execute(
            "UPDATE contact_device_registries SET version = ?1 WHERE contact_id = ?2",
            params![held_version as i64, contact.id()],
        )
        .unwrap();
    let held = storage
        .device()
        .load_contact_device_registry(contact.id())
        .unwrap()
        .unwrap();
    assert_eq!(held.version(), held_version);
    assert_eq!(held.active_devices()[0].notice_cohort, Some(7));
}
