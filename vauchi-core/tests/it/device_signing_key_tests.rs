// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Each device holds a random signing key, not derived from the identity seed
//! and in no backup (ADR-079 step 1, SC-62, vauchi/private#970).

use rusqlite::params;
use vauchi_core::api::AuthMode;
use vauchi_core::crypto::{SymmetricKey, encrypt};
use vauchi_core::identity::Identity;
use vauchi_core::storage::Storage;
use vauchi_core::{Vauchi, VauchiConfig, crypto::SigningKeyPair};

use crate::common::helpers::create_vauchi_with_identity;

/// The identity stored in `storage`, or one it does not hold when it has
/// none: what a caller that kept an identity in memory would pass.
fn stored_identity(storage: &Storage) -> Identity {
    match storage.identity().load_identity().unwrap() {
        Some(bytes) => Identity::from_storage_bytes(&bytes, 1_700_000_000).unwrap(),
        None => Identity::create("Nobody", 1_700_000_000),
    }
}

fn public_key_of(storage: &Storage) -> [u8; 32] {
    *storage
        .device()
        .load_or_create_device_signing_key(&stored_identity(storage))
        .unwrap()
        .public_key()
        .as_bytes()
}

fn open_storage() -> (tempfile::TempDir, std::path::PathBuf, SymmetricKey, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let storage = Storage::open(&path, key.clone()).unwrap();
    save_an_identity(&storage);
    (dir, path, key, storage)
}

fn save_an_identity(storage: &Storage) {
    storage
        .identity()
        .save(&Identity::create("Alice", 1_700_000_000))
        .unwrap();
}

fn stored_key_rows(storage: &Storage) -> i64 {
    storage
        .connection()
        .query_row("SELECT COUNT(*) FROM device_signing_key", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn creation_refusal(storage: &Storage) -> String {
    match storage
        .device()
        .load_or_create_device_signing_key(&stored_identity(storage))
    {
        Ok(_) => "a key was created".to_string(),
        Err(error) => error.to_string(),
    }
}

const NO_IDENTITY: &str =
    "Invalid data: a device signing key is created only while an identity is stored";

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

/// A worker that outlives an identity deletion must not leave a new key
/// behind for the next identity to sign with (ADR-079, vauchi/private#970).
// @internal
#[test]
fn no_key_is_created_in_a_store_without_an_identity() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();

    assert_eq!(creation_refusal(&storage), NO_IDENTITY);
    assert_eq!(stored_key_rows(&storage), 0);
}

const ANOTHER_IDENTITY: &str =
    "Invalid data: the stored device signing key belongs to another identity";

/// An install whose identity row went while its key stayed (an earlier
/// version, or a worker during a deletion) must not hand that key to the
/// next identity: one key under two identities links them (ADR-079,
/// vauchi/private#970).
// @internal
#[test]
fn a_new_identity_over_a_left_over_key_signs_with_a_fresh_one() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate());
    let mut first = Vauchi::new(config.clone()).unwrap();
    first.create_identity("Alice").unwrap();
    let left_over = public_key_of(first.storage());
    first.storage().identity().delete_identity().unwrap();
    drop(first);

    let mut next = Vauchi::new(config).unwrap();
    next.create_identity("Bob").unwrap();

    assert_ne!(public_key_of(next.storage()), left_over);
}

// @internal
#[test]
fn a_key_stored_for_another_identity_is_refused() {
    let (_dir, _path, _key, storage) = open_storage();
    public_key_of(&storage);
    let before = stored_row(&storage);

    storage
        .identity()
        .save(&Identity::create("Bob", 1_700_000_000))
        .unwrap();

    assert_eq!(creation_refusal(&storage), ANOTHER_IDENTITY);
    assert_eq!(stored_row(&storage), before);
}

// @internal
#[test]
fn adopting_a_key_without_a_stored_identity_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();

    let adopted = storage
        .device()
        .adopt_device_signing_key(
            &stored_identity(&storage),
            &zeroize::Zeroizing::new([0x0c; 32]),
        )
        .map_err(|error| error.to_string());

    assert_eq!(adopted, Err(NO_IDENTITY.to_string()));
    assert_eq!(stored_key_rows(&storage), 0);
}

/// Installs from before the key named its identity keep their key: their
/// contacts have pinned it.
// @internal
#[test]
fn a_key_stored_before_it_named_its_identity_keeps_its_public_key() {
    let (_dir, _path, key, storage) = open_storage();
    let seed = [0x2a; 32];
    storage
        .connection()
        .execute(
            "INSERT INTO device_signing_key (id, key_encrypted) VALUES (1, ?1)",
            params![encrypt(&key, &seed).unwrap()],
        )
        .unwrap();

    assert_eq!(
        public_key_of(&storage),
        *SigningKeyPair::from_seed(&seed).public_key().as_bytes()
    );
    storage
        .identity()
        .save(&Identity::create("Bob", 1_700_000_000))
        .unwrap();
    assert_eq!(creation_refusal(&storage), ANOTHER_IDENTITY);
}

// @internal
#[test]
fn an_identity_created_after_a_wipe_signs_with_a_fresh_key() {
    let mut wb = create_vauchi_with_identity("Alice");
    let wiped = public_key_of(wb.storage());
    wb.perform_emergency_wipe(true).unwrap();

    wb.create_identity("Bob").unwrap();

    assert_ne!(public_key_of(wb.storage()), wiped);
}

/// The app and its background worker open the same database.
// @internal
#[test]
fn two_connections_racing_to_create_the_key_get_the_same_one() {
    for _ in 0..20 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vauchi.db");
        let key = SymmetricKey::generate();
        // Both connections are opened before the race: two opens of one file
        // contend on the schema check behind a 5 s busy timeout, and twenty
        // such rounds under coverage on a loaded runner took longer than the
        // 180 s test timeout (private#1207). The race under test is the key
        // creation, which starts at the barrier.
        let first_storage = Storage::open(&path, key.clone()).unwrap();
        save_an_identity(&first_storage);
        let second_storage = Storage::open(&path, key.clone()).unwrap();
        let start = std::sync::Barrier::new(2);
        let start = &start;

        // Each connection moves into its own thread: a Storage is Send, not Sync.
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(move || {
                start.wait();
                public_key_of(&first_storage)
            });
            let second = scope.spawn(move || {
                start.wait();
                public_key_of(&second_storage)
            });
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
        .load_or_create_device_signing_key(&stored_identity(&storage))
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
    wb.setup_duress_pin_with_biometric_unlock("654321").unwrap();
    let before = public_key_of(wb.storage());

    let mode = wb.authenticate("654321").unwrap();
    // The real key is out of the duress foreground's reach (ADR-080).
    wb.authenticate("my-pin-1234").unwrap();

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
    public_key_of(wb.storage());
    assert_eq!(stored_key_rows(wb.storage()), 1);

    wb.perform_emergency_wipe(true).unwrap();

    assert_eq!(creation_refusal(wb.storage()), NO_IDENTITY);
    assert_eq!(stored_key_rows(wb.storage()), 0);
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

        let opened: Option<SigningKeyPair> = storage
            .device()
            .load_or_create_device_signing_key(&stored_identity(&storage))
            .ok();

        assert!(opened.is_none(), "{case} must be refused");
        assert_eq!(stored_row(&storage), row, "{case} must stay as it was");
    }
}
