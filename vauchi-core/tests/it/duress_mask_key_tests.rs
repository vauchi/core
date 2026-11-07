// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress content is sealed under the mask key, a key derived from the
//! storage key, not under the storage key itself (ADR-080 step 1,
//! vauchi/private#1117). The mask profile can then be handed its own key
//! without being handed the key that opens the real database.

use std::path::PathBuf;

use vauchi_core::crypto::{HKDF, decrypt, encrypt};
use vauchi_core::storage::Storage;
use vauchi_core::{ContactCard, SymmetricKey, Vauchi, VauchiConfig};

const MASK_KEY_INFO: &[u8] = b"Vauchi_Duress_Mask_Key_v1";
/// The padded length one decoy seals to: the first size bucket.
const SMALLEST_SEAL: usize = 4096;

struct OnDisk {
    _dir: tempfile::TempDir,
    path: PathBuf,
    key: SymmetricKey,
}

/// Alice with an app password, a duress PIN and one decoy contact.
fn alice_with_a_decoy() -> OnDisk {
    let dir = tempfile::tempdir().unwrap();
    let disk = OnDisk {
        path: dir.path().join("vauchi.db"),
        key: SymmetricKey::generate(),
        _dir: dir,
    };
    let config = VauchiConfig::with_storage_path(&disk.path).with_storage_key(disk.key.clone());
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password("my-pin-1234").unwrap();
    vauchi.setup_duress_password("654321").unwrap();
    vauchi
        .add_decoy_contact("decoy-1", "Sam", &ContactCard::new("Sam"))
        .unwrap();
    disk
}

fn mask_key(storage_key: &SymmetricKey) -> SymmetricKey {
    SymmetricKey::from_bytes(*HKDF::derive_key(
        None,
        storage_key.as_bytes(),
        MASK_KEY_INFO,
    ))
}

fn sealed_content(disk: &OnDisk) -> Vec<u8> {
    rusqlite::Connection::open(&disk.path)
        .unwrap()
        .query_row("SELECT content_encrypted FROM duress_content", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn decoy_names(disk: &OnDisk) -> Vec<String> {
    Storage::open(&disk.path, disk.key.clone())
        .unwrap()
        .decoy()
        .load_decoy_contacts()
        .unwrap()
        .into_iter()
        .map(|(_, name, _)| name)
        .collect()
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_duress_content_opens_under_the_mask_key_and_not_under_the_storage_key() {
    let disk = alice_with_a_decoy();

    let sealed = sealed_content(&disk);

    assert!(decrypt(&disk.key, &sealed).is_err());
    assert_eq!(
        decrypt(&mask_key(&disk.key), &sealed).unwrap().len(),
        SMALLEST_SEAL
    );
}

/// Puts the seal back as it was before v114, under the storage key.
fn seal_under_the_storage_key_as_before_v114(disk: &OnDisk) {
    let framed = decrypt(&mask_key(&disk.key), &sealed_content(disk)).unwrap();
    let conn = rusqlite::Connection::open(&disk.path).unwrap();
    conn.execute(
        "UPDATE duress_content SET content_encrypted = ?1",
        [encrypt(&disk.key, &framed).unwrap()],
    )
    .unwrap();
    conn.execute("DELETE FROM schema_version WHERE version >= 114", [])
        .unwrap();
}

// @scenario: duress_mode :: App update preserves duress configuration
#[test]
fn a_seal_from_before_v114_keeps_its_decoys_and_is_resealed_under_the_mask_key() {
    let disk = alice_with_a_decoy();
    seal_under_the_storage_key_as_before_v114(&disk);
    assert_eq!(
        decrypt(&disk.key, &sealed_content(&disk)).unwrap().len(),
        SMALLEST_SEAL
    );

    assert_eq!(decoy_names(&disk), ["Sam"]);

    let sealed = sealed_content(&disk);
    assert!(decrypt(&disk.key, &sealed).is_err());
    assert_eq!(
        decrypt(&mask_key(&disk.key), &sealed).unwrap().len(),
        SMALLEST_SEAL
    );
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_duress_content_survives_a_storage_rekey_under_the_new_mask_key() {
    let disk = alice_with_a_decoy();
    let new_key = SymmetricKey::generate();
    let mut storage = Storage::open(&disk.path, disk.key.clone()).unwrap();
    storage.rekey(new_key.clone()).unwrap();
    drop(storage);

    let sealed = sealed_content(&disk);
    assert_eq!(
        decrypt(&mask_key(&new_key), &sealed).unwrap().len(),
        SMALLEST_SEAL
    );
    assert!(decrypt(&new_key, &sealed).is_err());
}
