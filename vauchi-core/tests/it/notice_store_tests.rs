// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notice core's saved state at rest (vauchi/private#1375, step 1): one
//! sealed row per contact, kind and peer device, encrypted under a key of
//! that contact, versioned so that two connections cannot both advance a
//! direction (a reused chain position would make the receiver alarm), and
//! erased with the contact.
#![cfg(feature = "notice-core")]

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{NoticeStateKind, Storage, StorageError};

const DEVICE_A: [u8; 32] = [0xA1; 32];
const DEVICE_B: [u8; 32] = [0xB2; 32];
const MARKER: &[u8] = b"notice-state-plaintext-marker";

struct World {
    dir: tempfile::TempDir,
    key: SymmetricKey,
    contact: String,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let key = SymmetricKey::generate();
        let storage = Storage::open(dir.path().join("vauchi.db"), key.clone()).unwrap();
        let peer = Identity::create("Bob", 0);
        let contact = Contact::from_exchange(
            *peer.signing_public_key(),
            ContactCard::new("Bob"),
            SymmetricKey::generate(),
            0,
        );
        storage.contacts().save_contact(&contact).unwrap();
        Self {
            dir,
            key,
            contact: contact.id().to_string(),
        }
    }

    fn open(&self) -> Storage {
        Storage::open(self.dir.path().join("vauchi.db"), self.key.clone()).unwrap()
    }
}

fn state(byte: u8) -> Vec<u8> {
    let mut bytes = MARKER.to_vec();
    bytes.extend_from_slice(&[byte; 64]);
    bytes
}

// @internal
#[test]
fn a_saved_notice_state_loads_back_for_its_contact_kind_and_device() {
    let world = World::new();
    let storage = world.open();
    let notices = storage.notices();
    notices
        .save(
            &world.contact,
            NoticeStateKind::Sender,
            &DEVICE_A,
            &state(1),
        )
        .unwrap();
    notices
        .save(
            &world.contact,
            NoticeStateKind::Receiver,
            &DEVICE_A,
            &state(2),
        )
        .unwrap();
    notices
        .save(
            &world.contact,
            NoticeStateKind::Sender,
            &DEVICE_B,
            &state(3),
        )
        .unwrap();

    let load = |kind, device| {
        notices
            .load(&world.contact, kind, device)
            .unwrap()
            .map(|bytes| bytes.to_vec())
    };
    assert_eq!(load(NoticeStateKind::Sender, &DEVICE_A), Some(state(1)));
    assert_eq!(load(NoticeStateKind::Receiver, &DEVICE_A), Some(state(2)));
    assert_eq!(load(NoticeStateKind::Sender, &DEVICE_B), Some(state(3)));
    assert_eq!(load(NoticeStateKind::Receiver, &DEVICE_B), None);
}

// ADR-015: secrets at rest are encrypted; a notice state holds chain keys.
// @internal
#[test]
fn a_notice_state_is_not_readable_at_rest() {
    let world = World::new();
    world
        .open()
        .notices()
        .save(
            &world.contact,
            NoticeStateKind::Sender,
            &DEVICE_A,
            &state(1),
        )
        .unwrap();
    drop(world.open());

    let raw = std::fs::read(world.dir.path().join("vauchi.db")).unwrap();
    let wal = std::fs::read(world.dir.path().join("vauchi.db-wal")).unwrap_or_default();
    for bytes in [&raw, &wal] {
        assert!(
            !bytes.windows(MARKER.len()).any(|window| window == MARKER),
            "the plaintext reached the database file"
        );
    }
}

// Two connections (the unlocked app and a background run) must not both
// advance one direction: a save from a connection that read the state
// before another saved it is refused.
// @internal
#[test]
fn a_save_of_a_notice_state_another_connection_moved_is_refused() {
    let world = World::new();
    let first = world.open();
    first
        .notices()
        .save(
            &world.contact,
            NoticeStateKind::Sender,
            &DEVICE_A,
            &state(1),
        )
        .unwrap();
    let read_before = first
        .notices()
        .load(&world.contact, NoticeStateKind::Sender, &DEVICE_A)
        .unwrap();
    assert!(read_before.is_some());

    let second = world.open();
    second
        .notices()
        .load(&world.contact, NoticeStateKind::Sender, &DEVICE_A)
        .unwrap();
    second
        .notices()
        .save(
            &world.contact,
            NoticeStateKind::Sender,
            &DEVICE_A,
            &state(2),
        )
        .unwrap();

    let stale = first.notices().save(
        &world.contact,
        NoticeStateKind::Sender,
        &DEVICE_A,
        &state(3),
    );
    assert!(matches!(stale, Err(StorageError::NoticeMoved)), "{stale:?}");
    let kept = world
        .open()
        .notices()
        .load(&world.contact, NoticeStateKind::Sender, &DEVICE_A)
        .unwrap()
        .map(|bytes| bytes.to_vec());
    assert_eq!(kept, Some(state(2)), "the second connection's state stays");
}

// @internal
#[test]
fn deleting_a_contact_erases_its_notice_state() {
    let world = World::new();
    let storage = world.open();
    storage
        .notices()
        .save(
            &world.contact,
            NoticeStateKind::Sender,
            &DEVICE_A,
            &state(1),
        )
        .unwrap();
    storage
        .notices()
        .save(
            &world.contact,
            NoticeStateKind::Receiver,
            &DEVICE_B,
            &state(2),
        )
        .unwrap();

    storage.delete_contact(&world.contact).unwrap();

    for (kind, device) in [
        (NoticeStateKind::Sender, &DEVICE_A),
        (NoticeStateKind::Receiver, &DEVICE_B),
    ] {
        assert!(
            storage
                .notices()
                .load(&world.contact, kind, device)
                .unwrap()
                .is_none(),
            "{kind:?}"
        );
    }
}
