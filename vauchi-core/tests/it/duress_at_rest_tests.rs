// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Whether a duress PIN exists is not visible in the storage file without
//! the storage key (ADR-032, vauchi/private#947, SC-09). What an imager
//! reads is each row's shape: which columns are NULL, the plain integers,
//! and the length of every blob and text. Timestamps are left out; when a
//! row was written is a separate question (vauchi/private#1003).
//!
//! @scenario: duress_mode.feature :: The storage file does not reveal whether a duress PIN exists

use std::path::{Path, PathBuf};

use vauchi_core::api::AuthMode;
use vauchi_core::storage::Storage;
use vauchi_core::{AppPasswordConfig, ContactCard, SymmetricKey, Vauchi, VauchiConfig};

const APP_PASSWORD: &str = "my-pin-1234";
const DURESS_PIN: &str = "654321";

struct OnDisk {
    _dir: tempfile::TempDir,
    path: PathBuf,
    key: SymmetricKey,
}

impl OnDisk {
    fn open(&self) -> Vauchi {
        let config = VauchiConfig::with_storage_path(&self.path).with_storage_key(self.key.clone());
        Vauchi::new(config).unwrap()
    }
}

/// Alice with an app password and, when asked, a duress PIN.
fn alice(with_duress_pin: bool) -> OnDisk {
    let dir = tempfile::tempdir().unwrap();
    let disk = OnDisk {
        path: dir.path().join("vauchi.db"),
        key: SymmetricKey::generate(),
        _dir: dir,
    };
    let mut vauchi = disk.open();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password(APP_PASSWORD).unwrap();
    if with_duress_pin {
        vauchi.setup_duress_password(DURESS_PIN).unwrap();
    }
    disk
}

/// What a reader without the key sees of `table`: per row, each column's
/// NULL-ness, integer value or byte length, timestamps aside.
fn row_shapes(path: &Path, table: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
    let columns: Vec<String> = stmt.column_names().map(str::to_string).collect();
    stmt.query_map([], |row| {
        let mut shape = Vec::new();
        for (i, column) in columns.iter().enumerate() {
            if column.ends_with("_at") {
                continue;
            }
            let value = match row.get::<_, rusqlite::types::Value>(i)? {
                rusqlite::types::Value::Null => "NULL".to_string(),
                rusqlite::types::Value::Integer(n) => format!("{n}"),
                rusqlite::types::Value::Real(r) => format!("{r}"),
                rusqlite::types::Value::Text(t) => format!("text[{}]", t.len()),
                rusqlite::types::Value::Blob(b) => format!("blob[{}]", b.len()),
            };
            shape.push(format!("{column}={value}"));
        }
        Ok(shape.join(","))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

// @internal
#[test]
fn identity_rows_with_and_without_a_duress_pin_are_indistinguishable_without_the_smk() {
    let without = alice(false);
    let with = alice(true);

    assert_eq!(
        row_shapes(&with.path, "identity"),
        row_shapes(&without.path, "identity"),
        "the identity row tells a duress PIN apart"
    );
    // The own-card copy and the mask seed are inside the sealed row
    // (ADR-080), so its size says nothing either.
    assert_eq!(
        row_shapes(&with.path, "duress_content"),
        row_shapes(&without.path, "duress_content"),
        "the sealed duress row tells a duress PIN apart"
    );
}

// @internal
#[test]
fn a_duress_session_leaves_the_rows_shaped_as_without_a_duress_pin() {
    let without = alice(false);
    let with = alice(true);
    {
        let mut vauchi = with.open();
        assert_eq!(vauchi.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
        vauchi.own_card().unwrap();
    }

    for table in ["identity", "duress_content"] {
        assert_eq!(
            row_shapes(&with.path, table),
            row_shapes(&without.path, table),
            "{table} tells a duress session apart"
        );
    }
}

// @internal
#[test]
fn disabling_the_duress_pin_leaves_the_rows_shaped_as_without_one() {
    let without = alice(false);
    let with = alice(true);
    {
        let mut vauchi = with.open();
        vauchi.disable_duress().unwrap();
        assert!(!vauchi.is_duress_enabled().unwrap());
        assert!(
            vauchi.authenticate(DURESS_PIN).is_err(),
            "a disabled duress PIN still unlocks"
        );
    }

    for table in ["identity", "duress_content"] {
        assert_eq!(
            row_shapes(&with.path, table),
            row_shapes(&without.path, table),
            "{table} tells a disabled duress PIN apart"
        );
    }
}

/// The file's duress own-card table, which v93 fills and v114 drops.
const DURESS_OWN_CARD_BEFORE_V114: &str = "
    CREATE TABLE duress_own_card (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        card_json TEXT NOT NULL,
        card_json_encrypted BLOB,
        updated_at INTEGER NOT NULL
    );";

/// Rewrites Alice's password columns as core stored them before v93: the
/// duress hash alone under the key and a plain `duress_enabled` flag.
fn store_as_before_v93(disk: &OnDisk, with_duress_pin: bool) {
    let mut config = AppPasswordConfig::create(APP_PASSWORD).unwrap();
    if with_duress_pin {
        config.setup_duress(DURESS_PIN).unwrap();
    }
    let seal = |bytes: &[u8]| vauchi_core::crypto::encrypt(&disk.key, bytes).unwrap();
    let conn = rusqlite::Connection::open(&disk.path).unwrap();
    conn.execute(
        "UPDATE identity SET password_hash_encrypted = ?1, password_salt = ?2,
             duress_hash_encrypted = ?3, duress_salt = ?4, duress_enabled = ?5 WHERE id = 1",
        rusqlite::params![
            seal(config.password_hash()),
            config.password_salt().as_slice(),
            config.duress_hash().map(|h| seal(h)),
            config.duress_salt().map(|s| s.to_vec()),
            i64::from(with_duress_pin),
        ],
    )
    .unwrap();
    conn.execute_batch(DURESS_OWN_CARD_BEFORE_V114).unwrap();
    conn.execute("DELETE FROM schema_version WHERE version >= 93", [])
        .unwrap();
}

// @internal
#[test]
fn a_database_from_before_v93_is_sealed_and_keeps_its_duress_pin() {
    let without = alice(false);
    let legacy_with = alice(false);
    let legacy_without = alice(false);
    store_as_before_v93(&legacy_with, true);
    store_as_before_v93(&legacy_without, false);

    let mut vauchi = legacy_with.open();
    assert!(vauchi.is_duress_enabled().unwrap());
    assert_eq!(vauchi.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    drop(vauchi);
    let mut vauchi = legacy_without.open();
    assert!(!vauchi.is_duress_enabled().unwrap());
    assert_eq!(vauchi.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);
    drop(vauchi);

    for table in ["identity", "duress_content"] {
        assert_eq!(
            row_shapes(&legacy_with.path, table),
            row_shapes(&without.path, table),
            "{table} of a migrated duress database tells it apart"
        );
        assert_eq!(
            row_shapes(&legacy_without.path, table),
            row_shapes(&without.path, table),
            "{table} of a migrated database without duress differs from a new one"
        );
    }
}

/// Row counts of every table, the first thing an imager reads.
fn row_counts(path: &Path) -> Vec<(String, i64)> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    tables
        .into_iter()
        .map(|table| {
            let count = conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
                    row.get(0)
                })
                .unwrap();
            (table, count)
        })
        .collect()
}

// @internal
#[test]
fn a_duress_user_s_content_leaves_the_row_counts_as_without_a_duress_pin() {
    let without = alice(false);
    let with = alice(true);
    {
        let mut vauchi = with.open();
        assert_eq!(vauchi.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);
        vauchi
            .add_decoy_contact("decoy-1", "Sam", &ContactCard::new("Sam"))
            .unwrap();
    }
    {
        let mut vauchi = with.open();
        assert_eq!(vauchi.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
        vauchi.create_tag("Work").unwrap();
    }

    let counts_without = row_counts(&without.path);
    let differing: Vec<_> = row_counts(&with.path)
        .into_iter()
        .zip(&counts_without)
        .filter(|(with, without)| with != *without)
        .map(|((table, with), (_, without))| format!("{table}: {with} vs {without}"))
        .collect();
    assert!(
        differing.is_empty(),
        "row counts tell a duress user apart: {differing:?}"
    );
}

// @internal
#[test]
fn the_duress_content_survives_a_storage_rekey() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let new_key = SymmetricKey::generate();
    {
        let mut storage = Storage::open(&path, SymmetricKey::generate()).unwrap();
        storage
            .decoy()
            .save_decoy_contact("decoy-1", "Sam", &ContactCard::new("Sam"))
            .unwrap();
        storage.duress_tags().create_tag("Work").unwrap();
        storage.rekey(new_key.clone()).unwrap();
    }

    let storage = Storage::open(&path, new_key).unwrap();

    let decoys = storage.decoy().load_decoy_contacts().unwrap();
    assert_eq!(
        decoys
            .iter()
            .map(|(id, name, _)| (id.as_str(), name.as_str()))
            .collect::<Vec<_>>(),
        [("decoy-1", "Sam")]
    );
    let tags = storage.duress_tags().list_tags().unwrap();
    assert_eq!(
        tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Work"]
    );
    assert!(storage.tags().list_tags().unwrap().is_empty());
}

// @internal
#[test]
fn every_duress_store_s_writes_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    {
        let storage = Storage::open(&path, key.clone()).unwrap();
        storage
            .decoy()
            .save_decoy_contact("decoy-1", "Sam", &ContactCard::new("Sam"))
            .unwrap();
        storage.duress_tags().create_tag("Work").unwrap();
        storage
            .duress_places()
            .create_place("Bern", 46.95, 7.45)
            .unwrap();
        storage.duress_labels().create_group("Family").unwrap();
        storage
            .duress_labels()
            .save_contact_override("decoy-1", "phone", true)
            .unwrap();
    }

    let storage = Storage::open(&path, key).unwrap();

    assert_eq!(storage.decoy().load_decoy_contacts().unwrap().len(), 1);
    let tags = storage.duress_tags().list_tags().unwrap();
    assert_eq!(
        tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Work"]
    );
    let places = storage.duress_places().list_places().unwrap();
    assert_eq!(
        places.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Bern"]
    );
    let groups = storage.duress_labels().load_all_groups().unwrap();
    assert_eq!(
        groups.iter().map(|g| g.name()).collect::<Vec<_>>(),
        ["Family"]
    );
    let overrides = storage
        .duress_labels()
        .load_contact_overrides("decoy-1")
        .unwrap();
    assert_eq!(overrides.get("phone"), Some(&true));
    assert!(storage.tags().list_tags().unwrap().is_empty());
    assert!(storage.places().list_places().unwrap().is_empty());
}

/// Puts Alice's file back to v93: the five duress tables in the file, with
/// one row each, and no `duress_content`.
fn store_duress_content_as_before_v94(disk: &OnDisk) {
    let seal = |bytes: &[u8]| vauchi_core::crypto::encrypt(&disk.key, bytes).unwrap();
    let conn = rusqlite::Connection::open(&disk.path).unwrap();
    conn.execute_batch(
        "DROP TABLE duress_content;
         DELETE FROM schema_version WHERE version >= 94;
         CREATE TABLE decoy_contacts (id TEXT PRIMARY KEY, card_encrypted BLOB NOT NULL,
             created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, display_name_encrypted BLOB);
         CREATE TABLE duress_tags (id TEXT PRIMARY KEY, name_encrypted BLOB NOT NULL,
             contact_ids_json TEXT NOT NULL DEFAULT '[]', created_at INTEGER NOT NULL);
         CREATE TABLE duress_places (id TEXT PRIMARY KEY, data_encrypted BLOB NOT NULL,
             created_at INTEGER NOT NULL);
         CREATE TABLE duress_visibility_labels (id TEXT PRIMARY KEY, name_encrypted BLOB,
             name_hmac BLOB, contacts_json_encrypted BLOB, visible_fields_json_encrypted BLOB,
             display_name_override_encrypted BLOB, bio_override_encrypted BLOB,
             avatar_override_encrypted BLOB, created_at INTEGER NOT NULL, modified_at INTEGER NOT NULL);
         CREATE TABLE duress_contact_visibility_overrides (contact_id TEXT NOT NULL,
             field_id TEXT NOT NULL, is_visible INTEGER NOT NULL, PRIMARY KEY (contact_id, field_id));",
    )
    .unwrap();
    let card = serde_json::to_vec(&ContactCard::new("Sam")).unwrap();
    conn.execute(
        "INSERT INTO decoy_contacts VALUES ('decoy-1', ?1, 1, 1, ?2)",
        rusqlite::params![seal(&card), seal(b"Sam")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO duress_tags (id, name_encrypted, created_at) VALUES ('t1', ?1, 1)",
        [seal(b"Work")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO duress_contact_visibility_overrides VALUES ('decoy-1', 'phone', 1)",
        [],
    )
    .unwrap();
}

// @internal
#[test]
fn a_database_from_before_v94_keeps_its_duress_content_in_the_seal() {
    let without = alice(false);
    let legacy = alice(true);
    store_duress_content_as_before_v94(&legacy);

    let storage = Storage::open(&legacy.path, legacy.key.clone()).unwrap();
    let decoys = storage.decoy().load_decoy_contacts().unwrap();
    assert_eq!(
        decoys
            .iter()
            .map(|(id, name, _)| (id.as_str(), name.as_str()))
            .collect::<Vec<_>>(),
        [("decoy-1", "Sam")]
    );
    let tags = storage.duress_tags().list_tags().unwrap();
    assert_eq!(
        tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Work"]
    );
    let overrides = storage
        .duress_labels()
        .load_contact_overrides("decoy-1")
        .unwrap();
    assert_eq!(overrides.get("phone"), Some(&true));
    drop(storage);

    // Its seal and fields were under the storage key; the first open moves
    // both to the mask key (ADR-080, vauchi/private#1117).
    let sealed: Vec<u8> = rusqlite::Connection::open(&legacy.path)
        .unwrap()
        .query_row("SELECT content_encrypted FROM duress_content", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(vauchi_core::crypto::decrypt(&legacy.key, &sealed).is_err());
    let mask_key = crate::common::helpers::duress_mask_key(&legacy.key);
    assert_eq!(
        vauchi_core::crypto::decrypt(&mask_key, &sealed)
            .unwrap()
            .len(),
        4096
    );
    let reopened = Storage::open(&legacy.path, legacy.key.clone()).unwrap();
    assert_eq!(reopened.decoy().load_decoy_contacts().unwrap().len(), 1);
    drop(reopened);

    assert_eq!(row_counts(&legacy.path), row_counts(&without.path));
    assert_eq!(
        row_shapes(&legacy.path, "duress_content"),
        row_shapes(&without.path, "duress_content")
    );
}

// @internal
#[test]
fn a_duress_content_seal_that_does_not_open_refuses_the_storage() {
    let disk = alice(true);
    let conn = rusqlite::Connection::open(&disk.path).unwrap();
    let sealed: Vec<u8> = conn
        .query_row("SELECT content_encrypted FROM duress_content", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut tampered = sealed.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    conn.execute(
        "UPDATE duress_content SET content_encrypted = ?1",
        [&tampered],
    )
    .unwrap();
    drop(conn);

    assert!(matches!(
        Storage::open(&disk.path, disk.key.clone()),
        Err(vauchi_core::storage::StorageError::Encryption(_))
    ));
    let conn = rusqlite::Connection::open(&disk.path).unwrap();
    let after: Vec<u8> = conn
        .query_row("SELECT content_encrypted FROM duress_content", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        after, tampered,
        "a seal that does not open is left as it was"
    );
}

fn sealed_len(storage: &Storage) -> usize {
    storage
        .connection()
        .query_row(
            "SELECT length(content_encrypted) FROM duress_content",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap() as usize
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(24))]

    // @internal
    #[test]
    fn the_seal_has_a_bucket_s_length_and_reopens_to_the_same_tags(
        names in proptest::collection::vec("[a-z]{1,40}", 0..120),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vauchi.db");
        let key = SymmetricKey::generate();
        let empty_len = sealed_len(&Storage::in_memory(key.clone()).unwrap());
        let overhead = empty_len - 4096;
        let mut written = Vec::new();
        {
            let storage = Storage::open(&path, key.clone()).unwrap();
            for name in &names {
                written.push(storage.duress_tags().create_tag(name).unwrap().name);
            }
            let bucket = sealed_len(&storage) - overhead;
            proptest::prop_assert!(bucket >= 4096 && bucket.is_power_of_two(), "{bucket}");
        }

        let storage = Storage::open(&path, key).unwrap();
        let mut reopened: Vec<String> = storage
            .duress_tags()
            .list_tags()
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        reopened.sort();
        written.sort();
        proptest::prop_assert_eq!(reopened, written);
    }
}

// @internal
#[test]
fn a_decoy_contact_does_not_change_the_seal_s_length() {
    let empty = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let with_decoy = Storage::in_memory(SymmetricKey::generate()).unwrap();
    with_decoy
        .decoy()
        .save_decoy_contact("decoy-1", "Sam", &ContactCard::new("Sam"))
        .unwrap();

    assert_eq!(sealed_len(&with_decoy), sealed_len(&empty));
}

fn refused_by_the_seal<T: std::fmt::Debug>(
    result: Result<T, vauchi_core::storage::StorageError>,
) -> bool {
    matches!(result, Err(vauchi_core::storage::StorageError::Database(_)))
}

// @internal
#[test]
fn a_duress_write_whose_seal_fails_returns_the_error() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .connection()
        .execute_batch(
            "CREATE TRIGGER refuse_seal BEFORE UPDATE ON duress_content
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        )
        .unwrap();

    assert!(refused_by_the_seal(storage.decoy().save_decoy_contact(
        "decoy-1",
        "Sam",
        &ContactCard::new("Sam")
    )));
    assert!(refused_by_the_seal(
        storage.duress_tags().create_tag("Work")
    ));
    assert!(refused_by_the_seal(
        storage.duress_places().create_place("Bern", 46.95, 7.45)
    ));
    assert!(refused_by_the_seal(
        storage.duress_labels().create_group("Family")
    ));
    assert!(refused_by_the_seal(
        storage
            .duress_labels()
            .save_contact_override("decoy-1", "phone", true)
    ));
    assert!(
        storage.tags().create_tag("Work").is_ok(),
        "real stores do not seal"
    );
}
