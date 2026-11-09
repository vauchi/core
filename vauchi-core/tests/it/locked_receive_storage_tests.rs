// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A locked run opens the database with the Background Receive Key alone:
//! no storage key, so nothing under it can be read or written, by
//! construction (ADR-033 amendment 2026-11-07, vauchi/private#1093 slice 6,
//! step 1).
//!
//! Traces to: features/storage_key_upgrade.feature

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{LockedReceiveStorage, Storage, StorageError};

const MESSAGE: &[u8] = b"an update from the contact";

fn db(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("vauchi.db")
}

/// A ratchet pair: the contact's sending side and our receiving side.
fn pair() -> (DoubleRatchetState, DoubleRatchetState) {
    let secret = SymmetricKey::generate();
    let ours = X3DHKeyPair::generate();
    let theirs = DoubleRatchetState::initialize_initiator(&secret, *ours.public_key()).unwrap();
    (
        theirs,
        DoubleRatchetState::initialize_responder(&secret, ours),
    )
}

/// A contact row for the ratchet to belong to; returns its id.
fn contact_in(storage: &Storage) -> String {
    let peer = Identity::create("Bob", 0);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    contact.id().to_string()
}

fn unlocked(dir: &tempfile::TempDir, sek: &SymmetricKey, brk: &SymmetricKey) -> Storage {
    let mut storage = Storage::open(db(dir), sek.clone()).unwrap();
    storage.set_receive_key(brk.clone()).unwrap();
    storage
}

fn refusal(result: Result<LockedReceiveStorage, StorageError>) -> StorageError {
    match result {
        Ok(_) => panic!("the locked open succeeded"),
        Err(e) => e,
    }
}

// @internal
#[test]
fn a_locked_run_advances_a_ratchet_the_unlock_then_continues() {
    let dir = tempfile::tempdir().unwrap();
    let (sek, brk) = (SymmetricKey::generate(), SymmetricKey::generate());
    let (mut theirs, ours) = pair();
    let contact = {
        let storage = unlocked(&dir, &sek, &brk);
        let contact = contact_in(&storage);
        storage
            .ratchets()
            .save_ratchet_state(&contact, &ours, false)
            .unwrap();
        contact
    };

    let locked = LockedReceiveStorage::open(db(&dir), brk.clone()).unwrap();
    let (mut ours, is_initiator) = locked
        .ratchets()
        .load_ratchet_state(&contact)
        .unwrap()
        .unwrap();
    let first = ours.decrypt(&theirs.encrypt(MESSAGE).unwrap()).unwrap();
    locked
        .ratchets()
        .save_ratchet_state(&contact, &ours, is_initiator)
        .unwrap();
    drop(locked);

    let storage = unlocked(&dir, &sek, &brk);
    let (mut ours, _) = storage
        .ratchets()
        .load_ratchet_state(&contact)
        .unwrap()
        .unwrap();
    let second = ours.decrypt(&theirs.encrypt(MESSAGE).unwrap()).unwrap();

    assert_eq!(first, MESSAGE);
    assert_eq!(second, MESSAGE);
}

// @internal
#[test]
fn an_update_the_locked_run_put_in_the_inbox_opens_at_the_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let (sek, brk) = (SymmetricKey::generate(), SymmetricKey::generate());
    drop(unlocked(&dir, &sek, &brk));

    LockedReceiveStorage::open(db(&dir), brk.clone())
        .unwrap()
        .receive_inbox()
        .put(MESSAGE)
        .unwrap();

    let items = unlocked(&dir, &sek, &brk).receive_inbox().items().unwrap();
    assert_eq!(items.readable.len(), 1);
    assert_eq!(items.readable[0].update.as_slice(), MESSAGE);
}

// @internal
#[test]
fn the_locked_run_cannot_read_the_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let (sek, brk) = (SymmetricKey::generate(), SymmetricKey::generate());
    unlocked(&dir, &sek, &brk)
        .receive_inbox()
        .put(MESSAGE)
        .unwrap();

    let locked = LockedReceiveStorage::open(db(&dir), brk).unwrap();

    assert!(matches!(
        locked.receive_inbox().items(),
        Err(StorageError::StorageKeyUnavailable)
    ));
}

// @internal
#[test]
fn a_wrong_receive_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    drop(unlocked(
        &dir,
        &SymmetricKey::generate(),
        &SymmetricKey::generate(),
    ));

    let refused = refusal(LockedReceiveStorage::open(
        db(&dir),
        SymmetricKey::generate(),
    ));

    assert!(matches!(refused, StorageError::WrongKey), "{refused:?}");
}

// @internal
#[test]
fn a_file_never_given_a_receive_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    drop(Storage::open(db(&dir), SymmetricKey::generate()).unwrap());

    let refused = refusal(LockedReceiveStorage::open(
        db(&dir),
        SymmetricKey::generate(),
    ));

    assert!(
        matches!(refused, StorageError::ReceiveKeyUnavailable),
        "{refused:?}"
    );
}

// @internal
#[test]
fn a_file_not_yet_migrated_waits_for_the_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let brk = SymmetricKey::generate();
    drop(unlocked(&dir, &SymmetricKey::generate(), &brk));
    let newest: u32 = rusqlite::Connection::open(db(&dir))
        .unwrap()
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    rusqlite::Connection::open(db(&dir))
        .unwrap()
        .execute("DELETE FROM schema_version WHERE version = ?1", [newest])
        .unwrap();

    let refused = refusal(LockedReceiveStorage::open(db(&dir), brk));

    assert!(
        matches!(refused, StorageError::StorageKeyUnavailable),
        "{refused:?}"
    );
}

// @internal
#[test]
fn a_missing_file_is_not_created() {
    let dir = tempfile::tempdir().unwrap();

    let refused = refusal(LockedReceiveStorage::open(
        db(&dir),
        SymmetricKey::generate(),
    ));

    assert!(matches!(refused, StorageError::NotFound(_)), "{refused:?}");
    assert!(!db(&dir).exists());
}
