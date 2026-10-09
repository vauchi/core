// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A database opens only under the key its data is encrypted with
//! (vauchi/private#580). Without this check a wrong key opened silently and
//! failed later, row by row — the state an interrupted SMK migration leaves.

use std::path::Path;

use proptest::prelude::*;
use rusqlite::{Connection, params};
use vauchi_core::crypto::{SymmetricKey, encrypt};
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};
use vauchi_core::storage::{Storage, StorageError};

const LAST_VERSION_WITHOUT_KEY_CHECK: u32 = 74;

fn assert_wrong_key(result: Result<Storage, StorageError>) {
    match result {
        Err(StorageError::WrongKey) => {}
        Err(other) => panic!("expected WrongKey, got {other:?}"),
        Ok(_) => panic!("expected WrongKey, the database opened"),
    }
}

/// A database as an install before the key check left it: schema at
/// `LAST_VERSION_WITHOUT_KEY_CHECK`, optionally with an identity row.
fn unchecked_database(path: &Path, key: &SymmetricKey, with_identity: bool) {
    let conn = Connection::open(path).unwrap();
    let subset: Vec<_> = all_migrations()
        .iter()
        .filter(|m| m.version <= LAST_VERSION_WITHOUT_KEY_CHECK)
        .copied()
        .collect();
    MigrationRunner::run(&conn, key, &subset, None, 0).unwrap();
    if with_identity {
        conn.execute(
            "INSERT INTO identity (id, backup_data_encrypted, display_name, created_at)
             VALUES (1, ?1, 'Alice', 0)",
            params![encrypt(key, b"identity backup").unwrap()],
        )
        .unwrap();
    }
}

fn schema_version(path: &Path) -> u32 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
        .unwrap()
}

// @internal
#[test]
fn a_database_reopens_under_the_key_that_wrote_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    {
        let storage = Storage::open(&path, key.clone()).unwrap();
        storage.identity().save_identity(b"data").unwrap();
    }

    let storage = Storage::open(&path, key).unwrap();

    assert_eq!(
        storage.identity().load_identity().unwrap(),
        Some(b"data".to_vec())
    );
}

// @internal
#[test]
fn a_database_refuses_another_key_even_without_an_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    drop(Storage::open(&path, SymmetricKey::generate()).unwrap());

    assert_wrong_key(Storage::open(&path, SymmetricKey::generate()));
}

// @internal
#[test]
fn an_unchecked_database_is_refused_under_a_key_its_identity_does_not_decrypt_under() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    unchecked_database(&path, &key, true);

    assert_wrong_key(Storage::open(&path, SymmetricKey::generate()));

    assert_eq!(
        schema_version(&path),
        LAST_VERSION_WITHOUT_KEY_CHECK,
        "no migration may run under a key that does not open the data"
    );
    let storage = Storage::open(&path, key).unwrap();
    assert_eq!(
        storage.identity().load_identity().unwrap(),
        Some(b"identity backup".to_vec()),
        "the refused key must not have been recorded as the database's key"
    );
}

// @internal
#[test]
fn an_unchecked_database_gets_its_check_from_the_key_its_identity_decrypts_under() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    unchecked_database(&path, &key, true);
    drop(Storage::open(&path, key.clone()).unwrap());

    Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM identity", [])
        .unwrap();

    assert_wrong_key(Storage::open(&path, SymmetricKey::generate()));
    Storage::open(&path, key).unwrap();
}

// @internal
#[test]
fn an_unchecked_database_without_an_identity_takes_the_key_it_is_opened_with() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    unchecked_database(&path, &SymmetricKey::generate(), false);
    drop(Storage::open(&path, key.clone()).unwrap());

    assert_wrong_key(Storage::open(&path, SymmetricKey::generate()));
    Storage::open(&path, key).unwrap();
}

// @internal
#[test]
fn a_rekey_moves_the_check_to_the_new_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let old_key = SymmetricKey::generate();
    let new_key = SymmetricKey::generate();
    {
        let mut storage = Storage::open(&path, old_key.clone()).unwrap();
        storage.identity().save_identity(b"data").unwrap();
        storage.rekey(new_key.clone()).unwrap();
    }

    assert_wrong_key(Storage::open(&path, old_key));
    let storage = Storage::open(&path, new_key).unwrap();
    assert_eq!(
        storage.identity().load_identity().unwrap(),
        Some(b"data".to_vec())
    );
}

// @internal
#[test]
fn a_rekey_whose_commit_fails_leaves_the_old_key_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let old_key = SymmetricKey::generate();
    let new_key = SymmetricKey::generate();
    {
        let mut storage = Storage::open(&path, old_key.clone()).unwrap();
        storage.identity().save_identity(b"data").unwrap();
        storage.arm_commit_fault();

        assert!(matches!(
            storage.rekey(new_key.clone()),
            Err(StorageError::Database(_))
        ));
    }

    assert_wrong_key(Storage::open(&path, new_key));
    let storage = Storage::open(&path, old_key).unwrap();
    assert_eq!(
        storage.identity().load_identity().unwrap(),
        Some(b"data".to_vec())
    );
}

fn tamper_check(path: &Path, tamper: impl Fn(Vec<u8>) -> Vec<u8>) {
    let conn = Connection::open(path).unwrap();
    let sealed: Vec<u8> = conn
        .query_row("SELECT sealed_label FROM key_check", [], |r| r.get(0))
        .unwrap();
    conn.execute(
        "UPDATE key_check SET sealed_label = ?1",
        params![tamper(sealed)],
    )
    .unwrap();
}

// @internal
#[test]
fn a_tampered_check_fails_closed_under_the_right_key() {
    let tamperings: [(&str, fn(Vec<u8>) -> Vec<u8>); 4] = [
        ("empty", |_| Vec::new()),
        ("truncated", |mut s| {
            s.truncate(s.len() / 2);
            s
        }),
        ("last byte flipped", |mut s| {
            let last = s.len() - 1;
            s[last] ^= 0x01;
            s
        }),
        ("oversized", |mut s| {
            s.extend(std::iter::repeat_n(0u8, 64 * 1024));
            s
        }),
    ];
    for (name, tamper) in tamperings {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vauchi.db");
        let key = SymmetricKey::generate();
        drop(Storage::open(&path, key.clone()).unwrap());
        tamper_check(&path, tamper);

        let result = Storage::open(&path, key);

        assert!(
            matches!(result, Err(StorageError::WrongKey)),
            "{name}: expected WrongKey, got {:?}",
            result.map(|_| ())
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // @internal
    #[test]
    fn no_other_key_opens_a_checked_database(
        writer in any::<[u8; 32]>(),
        reader in any::<[u8; 32]>(),
    ) {
        prop_assume!(writer != reader);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vauchi.db");
        drop(Storage::open(&path, SymmetricKey::from_bytes(writer)).unwrap());

        let result = Storage::open(&path, SymmetricKey::from_bytes(reader));

        prop_assert!(matches!(result, Err(StorageError::WrongKey)));
    }
}
