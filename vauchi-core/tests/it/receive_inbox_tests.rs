// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A locked run seals each update it receives to the inbox public key; only
//! the storage key opens the inbox private key, so the inbox can be written
//! but not read while the phone is locked (ADR-033 addendum 2026-11-09,
//! point 1, vauchi/private#1093 slice 6).
//!
//! Traces to: features/storage_key_upgrade.feature

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use proptest::prelude::*;
use vauchi_core::crypto::{HKDF, SymmetricKey};
use vauchi_core::storage::{RECEIVE_INBOX_CAPACITY, Storage, StorageError};
use x25519_dalek::{PublicKey, StaticSecret};

const UPDATE: &[u8] = b"an update received while the phone was locked";

fn db(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("vauchi.db")
}

fn with_receive_key(dir: &tempfile::TempDir, sek: &SymmetricKey, brk: &SymmetricKey) -> Storage {
    let mut storage = Storage::open(db(dir), sek.clone()).unwrap();
    storage.set_receive_key(brk.clone()).unwrap();
    storage
}

fn readable(storage: &Storage) -> Vec<Vec<u8>> {
    storage
        .receive_inbox()
        .items()
        .unwrap()
        .readable
        .iter()
        .map(|item| item.update.to_vec())
        .collect()
}

// @internal
#[test]
fn updates_put_in_the_inbox_come_back_in_order_until_removed() {
    let dir = tempfile::tempdir().unwrap();
    let storage = with_receive_key(&dir, &SymmetricKey::generate(), &SymmetricKey::generate());

    storage.receive_inbox().put(b"first").unwrap();
    storage.receive_inbox().put(b"second").unwrap();
    let items = storage.receive_inbox().items().unwrap();
    storage
        .receive_inbox()
        .remove(items.readable[0].id)
        .unwrap();

    assert_eq!(
        items
            .readable
            .iter()
            .map(|item| item.update.to_vec())
            .collect::<Vec<_>>(),
        vec![b"first".to_vec(), b"second".to_vec()]
    );
    assert_eq!(readable(&storage), vec![b"second".to_vec()]);
}

// @internal
#[test]
fn the_inbox_needs_the_receive_key() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(db(&dir), SymmetricKey::generate()).unwrap();

    assert!(matches!(
        storage.receive_inbox().put(UPDATE),
        Err(StorageError::ReceiveKeyUnavailable)
    ));
}

// @internal
#[test]
fn the_inbox_reopens_with_the_same_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (sek, brk) = (SymmetricKey::generate(), SymmetricKey::generate());
    with_receive_key(&dir, &sek, &brk)
        .receive_inbox()
        .put(UPDATE)
        .unwrap();

    let reopened = with_receive_key(&dir, &sek, &brk);

    assert_eq!(readable(&reopened), vec![UPDATE.to_vec()]);
}

// @internal
#[test]
fn the_inbox_survives_a_storage_rekey() {
    let dir = tempfile::tempdir().unwrap();
    let brk = SymmetricKey::generate();
    let new_sek = SymmetricKey::generate();
    {
        let mut storage = with_receive_key(&dir, &SymmetricKey::generate(), &brk);
        storage.receive_inbox().put(UPDATE).unwrap();
        storage.rekey(new_sek.clone()).unwrap();
    }

    let reopened = with_receive_key(&dir, &new_sek, &brk);

    assert_eq!(readable(&reopened), vec![UPDATE.to_vec()]);
}

// @claim: S12
// @internal
#[test]
fn no_inbox_update_is_readable_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    {
        let storage = with_receive_key(&dir, &SymmetricKey::generate(), &SymmetricKey::generate());
        storage.receive_inbox().put(UPDATE).unwrap();
    }

    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes.windows(UPDATE.len()).any(|window| window == UPDATE),
            "a received update is sealed at rest"
        );
    }
}

// @internal
#[test]
fn a_damaged_item_is_reported_and_the_others_still_open() {
    let dir = tempfile::tempdir().unwrap();
    let storage = with_receive_key(&dir, &SymmetricKey::generate(), &SymmetricKey::generate());
    storage.receive_inbox().put(b"damaged").unwrap();
    storage.receive_inbox().put(UPDATE).unwrap();
    let damaged = storage.receive_inbox().items().unwrap().readable[0].id;
    rusqlite::Connection::open(db(&dir))
        .unwrap()
        .execute(
            "UPDATE receive_inbox SET sealed_update = zeroblob(80) WHERE id = ?1",
            [damaged],
        )
        .unwrap();

    let items = storage.receive_inbox().items().unwrap();

    assert_eq!(items.unreadable, vec![damaged]);
    assert_eq!(
        items
            .readable
            .iter()
            .map(|item| item.update.to_vec())
            .collect::<Vec<_>>(),
        vec![UPDATE.to_vec()]
    );
}

/// What someone who can write the file but holds no key can add: an item
/// sealed to the inbox public key stored beside it.
fn forge_item(dir: &tempfile::TempDir, update: &[u8]) {
    let conn = rusqlite::Connection::open(db(dir)).unwrap();
    let public_key: Vec<u8> = conn
        .query_row("SELECT public_key FROM inbox_key WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let public_key = PublicKey::from(<[u8; 32]>::try_from(public_key).unwrap());
    let ephemeral = StaticSecret::from([9; 32]);
    let shared = ephemeral.diffie_hellman(&public_key);
    let key = HKDF::derive_key(None, shared.as_bytes(), b"Vauchi_Receive_Inbox_Seal_v1");
    let nonce = [7; 24];
    let ciphertext = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .unwrap()
        .encrypt(&XNonce::from(nonce), update)
        .unwrap();
    let mut sealed = PublicKey::from(&ephemeral).as_bytes().to_vec();
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    conn.execute(
        "INSERT INTO receive_inbox (sealed_update) VALUES (?1)",
        [sealed],
    )
    .unwrap();
}

// @internal
#[test]
fn an_item_added_without_the_receive_key_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let storage = with_receive_key(&dir, &SymmetricKey::generate(), &SymmetricKey::generate());
    storage.receive_inbox().put(UPDATE).unwrap();
    forge_item(&dir, b"a forged update");

    let items = storage.receive_inbox().items().unwrap();

    assert_eq!(
        items
            .readable
            .iter()
            .map(|item| item.update.to_vec())
            .collect::<Vec<_>>(),
        vec![UPDATE.to_vec()]
    );
    assert_eq!(items.unreadable.len(), 1);
}

// @internal
#[test]
fn a_full_inbox_refuses_another_item_until_one_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let storage = with_receive_key(&dir, &SymmetricKey::generate(), &SymmetricKey::generate());
    for _ in 0..RECEIVE_INBOX_CAPACITY {
        storage.receive_inbox().put(UPDATE).unwrap();
    }

    let refused = storage.receive_inbox().put(UPDATE);
    let first = storage.receive_inbox().items().unwrap().readable[0].id;
    storage.receive_inbox().remove(first).unwrap();

    assert!(matches!(refused, Err(StorageError::ReceiveInboxFull)));
    assert!(matches!(storage.receive_inbox().put(UPDATE), Ok(())));
    assert_eq!(readable(&storage).len(), RECEIVE_INBOX_CAPACITY);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // @internal
    #[test]
    fn any_update_comes_back_unchanged(update in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            with_receive_key(&dir, &SymmetricKey::generate(), &SymmetricKey::generate());

        storage.receive_inbox().put(&update).unwrap();

        prop_assert_eq!(readable(&storage), vec![update]);
    }
}
