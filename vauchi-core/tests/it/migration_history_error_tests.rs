// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use rusqlite::Connection;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::StorageError;
use vauchi_core::storage::migration::{Migration, MigrationAction, MigrationRunner};

// @internal
#[test]
fn current_version_rejects_out_of_range_history() {
    for invalid in [-1_i64, i64::from(u32::MAX) + 1] {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)",
        )
        .unwrap();
        conn.execute("INSERT INTO schema_version VALUES (?1, 5)", [invalid])
            .unwrap();

        let error = MigrationRunner::current_version(&conn).unwrap_err();
        match error {
            StorageError::Database(rusqlite::Error::IntegralValueOutOfRange(column, value)) => {
                assert_eq!((column, value), (0, invalid));
            }
            other => panic!("expected a version range error, got {other:?}"),
        }
    }
}

// @internal
#[test]
fn current_version_reports_a_missing_history_column() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE schema_version (applied_at INTEGER NOT NULL)")
        .unwrap();

    let error = MigrationRunner::current_version(&conn).unwrap_err();
    match error {
        StorageError::Database(error) => {
            assert_eq!(error.sqlite_error().unwrap().extended_code, 1);
        }
        other => panic!("expected a schema query error, got {other:?}"),
    }
}

// @internal
#[test]
fn current_version_preserves_empty_and_valid_history_results() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 0);
    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)",
    )
    .unwrap();
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 0);
    for version in [1_u32, 17, u32::MAX] {
        conn.execute("INSERT INTO schema_version VALUES (?1, 5)", [version])
            .unwrap();
        assert_eq!(MigrationRunner::current_version(&conn).unwrap(), version);
    }
}

// @internal
#[test]
fn unreadable_history_prevents_migration_and_preserves_reopened_data() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("history.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
         INSERT INTO schema_version VALUES (-1, 5);
         CREATE TABLE retained_data (value TEXT NOT NULL);
         INSERT INTO retained_data VALUES ('kept');",
    )
    .unwrap();
    let key = SymmetricKey::generate();
    let migrations = [Migration {
        version: 1,
        name: "observable data migration",
        action: MigrationAction::Sql("UPDATE retained_data SET value = 'migrated'"),
    }];

    let result = MigrationRunner::run(&conn, &key, &migrations, None, 11);
    drop(conn);
    let conn = Connection::open(&path).unwrap();
    let value: String = conn
        .query_row("SELECT value FROM retained_data", [], |row| row.get(0))
        .unwrap();
    let history: Vec<(i64, i64)> = conn
        .prepare("SELECT version, applied_at FROM schema_version ORDER BY version")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!((value, history), ("kept".to_owned(), vec![(-1, 5)]));
    match result.unwrap_err() {
        StorageError::Database(rusqlite::Error::IntegralValueOutOfRange(column, value)) => {
            assert_eq!((column, value), (0, -1));
        }
        other => panic!("expected a version range error, got {other:?}"),
    }

    conn.execute("UPDATE schema_version SET version = 0", [])
        .unwrap();
    MigrationRunner::run(&conn, &key, &migrations, None, 11).unwrap();
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 1);
    let value: String = conn
        .query_row("SELECT value FROM retained_data", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "migrated");
}
