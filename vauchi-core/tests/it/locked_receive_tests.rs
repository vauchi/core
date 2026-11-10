// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A locked run opens a contact's blob on a session it already has, then
//! saves the advanced ratchet and the sealed inbox item in one transaction;
//! a blob no session opens is left on the relay for the unlock
//! (vauchi/private#1093 slice 6, steps 2 and 3).
//!
//! Traces to: features/storage_key_upgrade.feature

use vauchi_core::api::locked_receive::{LockedInboxItem, LockedReceipt, store_locked_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{LockedReceiveStorage, RECEIVE_INBOX_CAPACITY, Storage, StorageError};

const MESSAGE: &[u8] = b"an update from the contact";
const NOW: u64 = 1_760_000_000;
const SHARED: [u8; 32] = [0; 32];

struct World {
    dir: tempfile::TempDir,
    sek: SymmetricKey,
    brk: SymmetricKey,
    contact: String,
    theirs: DoubleRatchetState,
}

impl World {
    fn unlocked(&self) -> Storage {
        let mut storage = Storage::open(self.db(), self.sek.clone()).unwrap();
        storage.set_receive_key(self.brk.clone()).unwrap();
        storage
    }

    fn locked(&self) -> LockedReceiveStorage {
        LockedReceiveStorage::open(self.db(), self.brk.clone()).unwrap()
    }

    fn db(&self) -> std::path::PathBuf {
        self.dir.path().join("vauchi.db")
    }

    /// What the relay carries for the next message the contact sends.
    fn next_blob(&mut self) -> Vec<u8> {
        serde_json::to_vec(&self.theirs.encrypt(MESSAGE).unwrap()).unwrap()
    }

    fn inbox(&self) -> Vec<LockedInboxItem> {
        self.unlocked()
            .receive_inbox()
            .items()
            .unwrap()
            .readable
            .iter()
            .map(|item| LockedInboxItem::decode(&item.update).unwrap())
            .collect()
    }

    /// Whether the unlocked phone's session for `device` opens the
    /// contact's next message, that is, whether it is where the contact is.
    fn session_follows(&mut self, device: &[u8; 32]) -> bool {
        let message = self.theirs.encrypt(MESSAGE).unwrap();
        let (mut ours, _) = self
            .unlocked()
            .ratchets()
            .load_ratchet_state_for_device(&self.contact, device)
            .unwrap()
            .unwrap();
        ours.decrypt(&message).is_ok()
    }
}

/// A contact whose shared session (or the session with `device`) we hold.
fn world_with_session_on(device: [u8; 32]) -> World {
    let dir = tempfile::tempdir().unwrap();
    let (sek, brk) = (SymmetricKey::generate(), SymmetricKey::generate());
    let mut storage = Storage::open(dir.path().join("vauchi.db"), sek.clone()).unwrap();
    storage.set_receive_key(brk.clone()).unwrap();
    let peer = Identity::create("Bob", 0);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    let secret = SymmetricKey::generate();
    let ours = X3DHKeyPair::generate();
    let theirs = DoubleRatchetState::initialize_initiator(&secret, *ours.public_key()).unwrap();
    storage
        .ratchets()
        .save_ratchet_state_for_device(
            contact.id(),
            &device,
            &DoubleRatchetState::initialize_responder(&secret, ours),
            false,
        )
        .unwrap();
    World {
        dir,
        sek,
        brk,
        contact: contact.id().to_string(),
        theirs,
    }
}

// @internal
#[test]
fn a_blob_on_the_shared_session_is_stored_for_the_unlock() {
    let mut world = world_with_session_on(SHARED);
    let blob = world.next_blob();

    let receipt =
        store_locked_update(&world.locked(), "blob-1", &world.contact, &blob, NOW).unwrap();

    assert_eq!(receipt, LockedReceipt::Stored);
    let inbox = world.inbox();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].contact_id(), world.contact);
    assert_eq!(inbox[0].peer_device_id(), &SHARED);
    assert_eq!(inbox[0].plaintext(), MESSAGE);
    assert_eq!(inbox[0].received_at(), NOW);
    assert!(
        world.session_follows(&SHARED),
        "the ratchet was saved advanced"
    );
}

// @internal
#[test]
fn a_blob_is_left_for_the_unlock_when_the_inbox_is_full() {
    let mut world = world_with_session_on(SHARED);
    {
        let unlocked = world.unlocked();
        for _ in 0..RECEIVE_INBOX_CAPACITY {
            unlocked.receive_inbox().put(b"an earlier update").unwrap();
        }
    }
    let blob = world.next_blob();

    let receipt =
        store_locked_update(&world.locked(), "blob-1", &world.contact, &blob, NOW).unwrap();

    assert_eq!(receipt, LockedReceipt::LeftForUnlock);
    assert!(!world.unlocked().kept_blobs().was_kept("blob-1").unwrap());
    let (mut ours, _) = world
        .unlocked()
        .ratchets()
        .load_ratchet_state_for_device(&world.contact, &SHARED)
        .unwrap()
        .unwrap();
    let message = serde_json::from_slice(&blob).unwrap();
    assert_eq!(
        ours.decrypt(&message).unwrap(),
        MESSAGE,
        "the session did not move, so the unlock still opens the blob"
    );
}

// @internal
#[test]
fn a_blob_on_a_device_session_names_that_device() {
    let device = [7; 32];
    let mut world = world_with_session_on(device);
    let blob = world.next_blob();

    let receipt =
        store_locked_update(&world.locked(), "blob-1", &world.contact, &blob, NOW).unwrap();

    assert_eq!(receipt, LockedReceipt::Stored);
    assert_eq!(world.inbox()[0].peer_device_id(), &device);
}

// @internal
#[test]
fn a_blob_no_session_opens_is_left_and_changes_nothing() {
    let mut world = world_with_session_on(SHARED);
    let (mut stranger, _) = {
        let secret = SymmetricKey::generate();
        let other = X3DHKeyPair::generate();
        (
            DoubleRatchetState::initialize_initiator(&secret, *other.public_key()).unwrap(),
            other,
        )
    };
    let blob = serde_json::to_vec(&stranger.encrypt(MESSAGE).unwrap()).unwrap();

    let receipt =
        store_locked_update(&world.locked(), "blob-1", &world.contact, &blob, NOW).unwrap();

    assert_eq!(receipt, LockedReceipt::LeftForUnlock);
    assert!(world.inbox().is_empty());
    assert!(world.session_follows(&SHARED), "the session did not move");
}

// @internal
#[test]
fn a_contact_without_a_session_is_left_for_the_unlock() {
    let mut world = world_with_session_on(SHARED);
    let blob = world.next_blob();

    let receipt =
        store_locked_update(&world.locked(), "blob-1", "no-such-contact", &blob, NOW).unwrap();

    assert_eq!(receipt, LockedReceipt::LeftForUnlock);
    assert!(world.inbox().is_empty());
}

// @internal
#[test]
fn malformed_blobs_are_left_for_the_unlock() {
    let world = world_with_session_on(SHARED);
    let locked = world.locked();

    for blob in [
        b"".as_slice(),
        b"not json",
        b"{}",
        b"{\"header\":null}",
        &[0u8; 4096],
    ] {
        let receipt = store_locked_update(&locked, "blob-1", &world.contact, blob, NOW).unwrap();
        assert_eq!(receipt, LockedReceipt::LeftForUnlock, "{blob:?}");
    }
    assert!(world.inbox().is_empty());
}

// @internal
#[test]
fn when_the_inbox_write_fails_the_ratchet_does_not_move() {
    let mut world = world_with_session_on(SHARED);
    let blob = world.next_blob();
    rusqlite::Connection::open(world.db())
        .unwrap()
        .execute("DELETE FROM inbox_key", [])
        .unwrap();

    let failed = store_locked_update(&world.locked(), "blob-1", &world.contact, &blob, NOW);

    assert!(
        matches!(failed, Err(StorageError::ReceiveKeyUnavailable)),
        "{:?}",
        failed.map(|_| ())
    );
    let (mut ours, _) = world
        .locked()
        .ratchets()
        .load_ratchet_state(&world.contact)
        .unwrap()
        .unwrap();
    let blob: vauchi_core::crypto::RatchetMessage = serde_json::from_slice(&blob).unwrap();
    assert_eq!(
        ours.decrypt(&blob).unwrap(),
        MESSAGE,
        "the saved session still opens the blob that was not stored"
    );
}

// @internal
#[test]
fn an_unlocked_save_of_a_session_a_locked_run_moved_is_refused() {
    let mut world = world_with_session_on(SHARED);
    let unlocked = world.unlocked();
    let (read_before, is_initiator) = unlocked
        .ratchets()
        .load_ratchet_state_for_device(&world.contact, &SHARED)
        .unwrap()
        .unwrap();
    let blob = world.next_blob();
    let receipt =
        store_locked_update(&world.locked(), "blob-1", &world.contact, &blob, NOW).unwrap();

    let stale_save = unlocked.ratchets().save_ratchet_state_for_device(
        &world.contact,
        &SHARED,
        &read_before,
        is_initiator,
    );

    assert_eq!(receipt, LockedReceipt::Stored);
    assert!(
        matches!(stale_save, Err(StorageError::RatchetMoved)),
        "{stale_save:?}"
    );
    let (mut kept, _) = world
        .unlocked()
        .ratchets()
        .load_ratchet_state_for_device(&world.contact, &SHARED)
        .unwrap()
        .unwrap();
    let message = serde_json::from_slice(&blob).unwrap();
    assert!(
        kept.decrypt(&message).is_err(),
        "the stored session is the locked run's, already past the blob"
    );
}

// @internal
#[test]
fn a_session_read_again_after_a_locked_run_saves() {
    let mut world = world_with_session_on(SHARED);
    let unlocked = world.unlocked();
    unlocked
        .ratchets()
        .load_ratchet_state_for_device(&world.contact, &SHARED)
        .unwrap();
    let blob = world.next_blob();
    store_locked_update(&world.locked(), "blob-1", &world.contact, &blob, NOW).unwrap();

    let (current, is_initiator) = unlocked
        .ratchets()
        .load_ratchet_state_for_device(&world.contact, &SHARED)
        .unwrap()
        .unwrap();

    for _ in 0..2 {
        assert!(matches!(
            unlocked.ratchets().save_ratchet_state_for_device(
                &world.contact,
                &SHARED,
                &current,
                is_initiator,
            ),
            Ok(())
        ));
    }
}
