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
    let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
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
    assert_eq!(
        row_shapes(&with.path, "duress_own_card"),
        row_shapes(&without.path, "duress_own_card"),
        "the duress own-card row tells a duress PIN apart"
    );
    assert_eq!(
        row_shapes(&without.path, "duress_own_card").len(),
        1,
        "every identity with an app password has the duress own-card row"
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

    for table in ["identity", "duress_own_card"] {
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

    for table in ["identity", "duress_own_card"] {
        assert_eq!(
            row_shapes(&with.path, table),
            row_shapes(&without.path, table),
            "{table} tells a disabled duress PIN apart"
        );
    }
}

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
    conn.execute("DELETE FROM duress_own_card", []).unwrap();
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

    for table in ["identity", "duress_own_card"] {
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
