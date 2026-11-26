// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Capability withdrawals survive newer global versions and concurrent writers.

use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use vauchi_core::card::ContactCard;
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::contact::Contact;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceRegistry, Identity, RegistryBroadcast};
use vauchi_core::storage::{Storage, StorageError};

const NOW: u64 = 1_800_000_000;

struct World {
    directory: tempfile::TempDir,
    key: SymmetricKey,
    storage: Storage,
    owner: Identity,
    device: SigningKeyPair,
    registry: DeviceRegistry,
    contact: String,
}

impl World {
    fn new() -> Self {
        let owner = Identity::create("Alice", NOW);
        let device = SigningKeyPair::generate();
        let registry = DeviceRegistry::new(
            owner
                .device_info()
                .to_registered(owner.master_seed())
                .with_signing_key(*device.public_key().as_bytes()),
            owner.signing_keypair(),
        );
        let directory = tempfile::tempdir().unwrap();
        let key = SymmetricKey::generate();
        let storage = Storage::open(directory.path().join("vauchi.db"), key.clone())
            .unwrap()
            .with_clock(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)).shared());
        let contact = Contact::from_exchange(
            *owner.signing_public_key(),
            ContactCard::new("Alice"),
            SymmetricKey::generate(),
            NOW,
        );
        storage.contacts().save_contact(&contact).unwrap();
        Self {
            directory,
            key,
            storage,
            owner,
            device,
            registry,
            contact: contact.id().to_string(),
        }
    }

    fn broadcast(&self, registry: &DeviceRegistry) -> RegistryBroadcast {
        RegistryBroadcast::signed_by_device(
            registry,
            self.owner.signing_keypair(),
            *self.owner.device_id(),
            &self.device,
            NOW,
        )
    }

    fn save(&self, registry: &DeviceRegistry) -> Result<(), StorageError> {
        self.storage.device().save_contact_device_registry(
            &self.contact,
            &self.broadcast(registry),
            self.owner.signing_public_key(),
            60,
        )
    }

    fn withdraw(&mut self) -> DeviceRegistry {
        self.registry
            .set_notice_cohort(
                self.owner.device_id(),
                Some(7),
                self.owner.signing_keypair(),
            )
            .unwrap();
        self.save(&self.registry).unwrap();
        let stale = self.registry.clone();
        self.registry
            .set_notice_cohort(self.owner.device_id(), None, self.owner.signing_keypair())
            .unwrap();
        self.save(&self.registry).unwrap();
        stale
    }

    fn held(&self) -> RegistryBroadcast {
        self.storage
            .device()
            .load_contact_device_registry(&self.contact)
            .unwrap()
            .unwrap()
    }
}

// @internal
#[test]
fn a_newer_global_version_cannot_restore_a_stale_cohort_announcement() {
    let mut world = World::new();
    let mut stale = world.withdraw();
    stale.lift_version_to(world.registry.version() + 1, world.owner.signing_keypair());
    assert!(matches!(
        world.save(&stale),
        Err(StorageError::InvalidData(_))
    ));
    let held = world.held();
    assert_eq!(held.version(), world.registry.version());
    assert_eq!(held.active_devices()[0].notice_cohort, None);
    assert_eq!(held.active_devices()[0].notice_capability_version, 2);
}

// @internal
#[test]
fn an_equal_revision_cannot_override_a_held_withdrawal_but_a_new_revision_can() {
    let mut world = World::new();
    world.withdraw();
    let mut device = world
        .registry
        .find_device(world.owner.device_id())
        .unwrap()
        .clone();
    device.notice_cohort = Some(9);
    let mut conflict = DeviceRegistry::new(device, world.owner.signing_keypair());
    conflict.lift_version_to(world.registry.version() + 1, world.owner.signing_keypair());
    assert!(matches!(
        world.save(&conflict),
        Err(StorageError::InvalidData(_))
    ));
    assert_eq!(world.held().active_devices()[0].notice_cohort, None);
    world
        .registry
        .set_notice_cohort(
            world.owner.device_id(),
            Some(9),
            world.owner.signing_keypair(),
        )
        .unwrap();
    world.save(&world.registry).unwrap();
    let held = world.held();
    assert_eq!(held.active_devices()[0].notice_cohort, Some(9));
    assert_eq!(held.active_devices()[0].notice_capability_version, 3);
}

struct AfterVersionRead {
    barrier: Arc<Barrier>,
    waited: AtomicBool,
}

impl Clock for AfterVersionRead {
    fn now(&self) -> SystemTime {
        // The acceptance API asks the clock after reading the stored version.
        if !self.waited.swap(true, Ordering::SeqCst) {
            self.barrier.wait();
        }
        UNIX_EPOCH + Duration::from_secs(NOW)
    }
}

// @internal
#[test]
fn only_one_registry_writer_can_commit_from_the_same_observed_version() {
    let mut world = World::new();
    world.withdraw();
    world
        .registry
        .set_notice_cohort(
            world.owner.device_id(),
            Some(9),
            world.owner.signing_keypair(),
        )
        .unwrap();
    let first = world.broadcast(&world.registry);
    world
        .registry
        .set_notice_cohort(world.owner.device_id(), None, world.owner.signing_keypair())
        .unwrap();
    let second = world.broadcast(&world.registry);
    let expected = [first.clone(), second.clone()];
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = [first, second]
        .into_iter()
        .map(|broadcast| {
            let path = world.directory.path().join("vauchi.db");
            let key = world.key.clone();
            let contact = world.contact.clone();
            let owner = *world.owner.signing_public_key();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let storage =
                    Storage::open(path, key)
                        .unwrap()
                        .with_clock(Arc::new(AfterVersionRead {
                            barrier,
                            waited: AtomicBool::new(false),
                        }));
                let result = storage
                    .device()
                    .save_contact_device_registry(&contact, &broadcast, &owner, 60);
                assert!(matches!(result, Ok(()) | Err(StorageError::InvalidData(_))));
                (broadcast.version(), result.is_ok())
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|(_, accepted)| *accepted).count(), 1);
    let version = results.iter().find(|(_, accepted)| *accepted).unwrap().0;
    let held = world.held();
    let accepted = expected
        .iter()
        .find(|broadcast| broadcast.version() == version)
        .unwrap();
    assert_eq!(held.version(), version);
    assert_eq!(
        held.active_devices()[0].notice_cohort,
        accepted.active_devices()[0].notice_cohort
    );
    assert_eq!(
        held.active_devices()[0].notice_capability_version,
        accepted.active_devices()[0].notice_capability_version
    );
}
