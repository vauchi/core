// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use rusqlite::Connection;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::StorageError;
use vauchi_core::storage::migration::{Migration, MigrationAction, MigrationRunner};

#[test]
fn migration_commit_uses_full_sync_and_restores_normal_after_success() {
    let root = tempfile::tempdir().unwrap();
    let conn = Connection::open(root.path().join("fixture.db")).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL")
        .unwrap();
    let migration = Migration {
        version: 1,
        name: "observe migration durability",
        action: MigrationAction::Sql(
            "CREATE TABLE observed_sync(value INTEGER); INSERT INTO observed_sync SELECT synchronous FROM pragma_synchronous",
        ),
    };
    MigrationRunner::run(&conn, &SymmetricKey::generate(), &[migration], None, 11).unwrap();
    assert_eq!(
        conn.query_row("SELECT value FROM observed_sync", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

fn abort(conn: &Connection, _: &SymmetricKey) -> Result<(), StorageError> {
    let policy: i64 = conn.pragma_query_value(None, "synchronous", |r| r.get(0))?;
    let reason = if policy == 2 {
        "controlled abort"
    } else {
        "unsafe migration sync"
    };
    Err(StorageError::InvalidData(reason.into()))
}

#[test]
fn failed_migration_restores_the_previous_sync_policy() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA synchronous=NORMAL").unwrap();
    let migration = Migration {
        version: 1,
        name: "abort fixture",
        action: MigrationAction::Callback(abort),
    };
    let error =
        MigrationRunner::run(&conn, &SymmetricKey::generate(), &[migration], None, 11).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Migration error: Migration v1 'abort fixture' callback failed: Invalid data: controlled abort"
    );
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 0);
    assert_eq!(
        conn.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
