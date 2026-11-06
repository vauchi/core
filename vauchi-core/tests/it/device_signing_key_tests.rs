// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Each device holds a random signing key, not derived from the identity seed
//! and in no backup (ADR-079 step 1, SC-62, vauchi/private#970).

use rusqlite::params;
use vauchi_core::api::AuthMode;
use vauchi_core::crypto::{SymmetricKey, encrypt};
use vauchi_core::storage::Storage;
use vauchi_core::{Vauchi, crypto::SigningKeyPair};

use crate::common::helpers::create_vauchi_with_identity;

fn public_key_of(storage: &Storage) -> [u8; 32] {
    *storage
        .device()
        .load_or_create_device_signing_key()
        .unwrap()
        .public_key()
        .as_bytes()
}

fn open_storage() -> (tempfile::TempDir, std::path::PathBuf, SymmetricKey, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let storage = Storage::open(&path, key.clone()).unwrap();
    (dir, path, key, storage)
}

fn stored_row(storage: &Storage) -> Vec<u8> {
    storage
        .connection()
        .query_row(
            "SELECT key_encrypted FROM device_signing_key WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

// @internal
#[test]
fn the_device_key_is_not_the_identity_key() {
    let wb = create_vauchi_with_identity("Alice");

    let device_key = public_key_of(wb.storage());

    assert_ne!(
        &device_key,
        wb.identity().unwrap().signing_public_key(),
        "a key the seed derives would attribute nothing"
    );
}

// @internal
#[test]
fn two_devices_with_one_seed_hold_different_keys() {
    let wb = create_vauchi_with_identity("Alice");
    let backup = wb.export_backup("correct horse battery").unwrap();
    let mut restored: Vauchi = Vauchi::in_memory().unwrap();
    restored
        .import_backup(&backup, "correct horse battery")
        .unwrap();
    assert_eq!(
        wb.identity().unwrap().signing_public_key(),
        restored.identity().unwrap().signing_public_key(),
        "both devices must hold the same identity for this test to mean anything"
    );

    assert_ne!(
        public_key_of(wb.storage()),
        public_key_of(restored.storage())
    );
}

// @internal
#[test]
fn a_backup_imported_over_another_identity_takes_a_new_key() {
    let mut wb = create_vauchi_with_identity("Alice");
    let before = public_key_of(wb.storage());
    let backup = create_vauchi_with_identity("Bob")
        .export_backup("correct horse battery")
        .unwrap();

    wb.import_backup(&backup, "correct horse battery").unwrap();

    assert_ne!(public_key_of(wb.storage()), before);
}

/// The app and its background worker open the same database.
// @internal
#[test]
fn two_connections_racing_to_create_the_key_get_the_same_one() {
    for _ in 0..20 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vauchi.db");
        let key = SymmetricKey::generate();
        drop(Storage::open(&path, key.clone()).unwrap());
        let start = std::sync::Barrier::new(2);

        let (first, second) = std::thread::scope(|scope| {
            let race = || {
                let storage = Storage::open(&path, key.clone()).unwrap();
                start.wait();
                public_key_of(&storage)
            };
            let first = scope.spawn(race);
            let second = scope.spawn(race);
            (first.join().unwrap(), second.join().unwrap())
        });

        assert_eq!(first, second);
    }
}

// @internal
#[test]
fn the_key_survives_a_restart() {
    let (_dir, path, key, storage) = open_storage();
    let before = public_key_of(&storage);
    drop(storage);

    let reopened = Storage::open(&path, key).unwrap();

    assert_eq!(public_key_of(&reopened), before);
}

// @internal
#[test]
fn the_stored_key_signs_under_its_public_key() {
    let (_dir, _path, _key, storage) = open_storage();
    let device_key = storage
        .device()
        .load_or_create_device_signing_key()
        .unwrap();

    let signature = device_key.sign(b"a registry broadcast");

    assert!(
        device_key
            .public_key()
            .verify(b"a registry broadcast", &signature)
    );
    assert!(
        !device_key
            .public_key()
            .verify(b"another broadcast", &signature)
    );
}

// @internal
#[test]
fn the_key_survives_a_storage_rekey() {
    let (_dir, _path, _key, mut storage) = open_storage();
    let before = public_key_of(&storage);

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(public_key_of(&storage), before);
}

/// A duress session can owe a registry push (`queue_registry_pushes` has no
/// duress branch), and any change here would show to contacts (ADR-032).
// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn a_duress_session_holds_the_same_key() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234").unwrap();
    wb.setup_duress_password("654321").unwrap();
    let before = public_key_of(wb.storage());

    let mode = wb.authenticate("654321").unwrap();

    assert_eq!(mode, AuthMode::Duress);
    assert_eq!(public_key_of(wb.storage()), before);
}

// @internal
#[test]
fn wiping_device_data_forgets_the_key() {
    let (_dir, _path, _key, storage) = open_storage();
    let before = public_key_of(&storage);

    storage.wipe_device_data().unwrap();

    assert_ne!(public_key_of(&storage), before);
}

// @internal
#[test]
fn an_emergency_wipe_of_an_in_memory_store_forgets_the_key() {
    let mut wb = create_vauchi_with_identity("Alice");
    let before = public_key_of(wb.storage());

    wb.perform_emergency_wipe(true).unwrap();

    assert_ne!(public_key_of(wb.storage()), before);
}

/// Contacts pin this key and never accept another for the device, so a row
/// that does not open must fail loudly rather than be replaced by a new key.
// @internal
#[test]
fn a_row_that_does_not_open_is_refused_and_kept() {
    let (_dir, _path, key, storage) = open_storage();
    let other_key = SymmetricKey::generate();
    let rows: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("not ciphertext", vec![0xFF; 64]),
        ("another key", encrypt(&other_key, &[7; 32]).unwrap()),
        ("no plaintext", encrypt(&key, &[]).unwrap()),
        ("short plaintext", encrypt(&key, &[7; 31]).unwrap()),
        ("long plaintext", encrypt(&key, &[7; 33]).unwrap()),
    ];
    public_key_of(&storage);

    for (case, row) in rows {
        storage
            .connection()
            .execute(
                "UPDATE device_signing_key SET key_encrypted = ?1 WHERE id = 1",
                params![row],
            )
            .unwrap();

        let opened: Option<SigningKeyPair> =
            storage.device().load_or_create_device_signing_key().ok();

        assert!(opened.is_none(), "{case} must be refused");
        assert_eq!(stored_row(&storage), row, "{case} must stay as it was");
    }
}
