// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::migration::{Migration, MigrationAction, MigrationRunner};

#[derive(Clone, Default)]
struct Diagnostics(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

impl Subscriber for Diagnostics {
    // #1175: scoped observers change during parallel tests; keep interest dynamic.
    fn register_callsite(&self, _: &'static Metadata<'static>) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn max_level_hint(&self) -> Option<tracing::metadata::LevelFilter> {
        Some(tracing::metadata::LevelFilter::TRACE)
    }
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields(BTreeMap::new());
        event.record(&mut fields);
        fields
            .0
            .insert("level".into(), event.metadata().level().to_string());
        fields
            .0
            .insert("target".into(), event.metadata().target().into());
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

fn seed(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
         INSERT INTO schema_version VALUES (1, 5);
         CREATE TABLE retained_data (value TEXT NOT NULL);
         INSERT INTO retained_data VALUES ('kept');",
    )
    .unwrap();
}

fn data(conn: &Connection) -> String {
    conn.query_row("SELECT value FROM retained_data", [], |row| row.get(0))
        .unwrap()
}

fn update() -> Migration {
    Migration {
        version: 2,
        name: "observable data migration",
        action: MigrationAction::Sql("UPDATE retained_data SET value = 'migrated'"),
    }
}

// @internal
#[test]
fn backup_diagnostics_are_silent_on_success_and_redacted_on_failure() {
    let diagnostics = Diagnostics::default();
    // #1175: one observer lifetime avoids concurrent subscription registration.
    tracing::subscriber::with_default(diagnostics.clone(), || {
        assert_successful_snapshot();
        assert_eq!(*diagnostics.0.lock().unwrap(), Vec::new());
        assert_backup_failure(&diagnostics);
    });
}

fn assert_successful_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let conn = Connection::open(&path).unwrap();
    seed(&conn);
    let key = SymmetricKey::generate();
    MigrationRunner::run(&conn, &key, &[update()], Some(&path), 11).unwrap();
    assert_eq!(data(&conn), "migrated");
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 2);

    let snapshot = Connection::open_with_flags(
        path.with_extension("pre-migration-v1.bak"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(data(&snapshot), "kept");
    assert_eq!(MigrationRunner::current_version(&snapshot).unwrap(), 1);
}

fn assert_backup_failure(diagnostics: &Diagnostics) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private-person-name.db");
    let conn = Connection::open(&path).unwrap();
    seed(&conn);
    let backup = path.with_extension("pre-migration-v1.bak");
    let recovery = Connection::open(&backup).unwrap();
    seed(&recovery);
    drop(recovery);
    let recovery_bytes = std::fs::read(&backup).unwrap();
    let backend_error = conn
        .execute(
            "VACUUM INTO ?1",
            rusqlite::params![backup.to_string_lossy().as_ref()],
        )
        .unwrap_err();
    let backend_code = match backend_error {
        rusqlite::Error::SqliteFailure(error, _) | rusqlite::Error::SqlInputError { error, .. } => {
            error.extended_code
        }
        other => panic!("expected a SQLite backup error, got {other:?}"),
    };
    let key = SymmetricKey::generate();
    MigrationRunner::run(&conn, &key, &[update()], Some(&path), 11).unwrap();

    assert_eq!(data(&conn), "migrated");
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 2);
    assert_eq!(std::fs::read(&backup).unwrap(), recovery_bytes);
    let expected = BTreeMap::from([
        ("level".into(), "WARN".into()),
        ("target".into(), "vauchi.storage.migration".into()),
        ("sqlite_error_code".into(), format!("Some({backend_code})")),
        (
            "message".into(),
            "pre-migration backup unavailable; continuing transactionally".into(),
        ),
    ]);
    assert_eq!(*diagnostics.0.lock().unwrap(), vec![expected]);
}

// @internal
#[test]
fn rollback_preserves_data_even_when_no_recovery_snapshot_can_be_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let conn = Connection::open(&path).unwrap();
    seed(&conn);
    let backup = path.with_extension("pre-migration-v1.bak");
    std::fs::create_dir(&backup).unwrap();
    let fail = Migration {
        version: 3,
        name: "controlled failure without a backup",
        action: MigrationAction::Sql("INSERT INTO missing_table VALUES (1)"),
    };
    let error = MigrationRunner::run(
        &conn,
        &SymmetricKey::generate(),
        &[update(), fail],
        Some(&path),
        11,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        vauchi_core::storage::StorageError::Migration(_)
    ));
    drop(conn);
    let reopened = Connection::open(&path).unwrap();
    assert_eq!(data(&reopened), "kept");
    assert_eq!(MigrationRunner::current_version(&reopened).unwrap(), 1);
    assert!(backup.is_dir());
    assert_eq!(std::fs::read_dir(&backup).unwrap().count(), 0);
}

// @internal
#[test]
fn migration_failure_rolls_back_data_and_version_independently_of_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let conn = Connection::open(&path).unwrap();
    seed(&conn);
    let key = SymmetricKey::generate();
    let fail = Migration {
        version: 3,
        name: "controlled failure",
        action: MigrationAction::Sql("INSERT INTO missing_table VALUES (1)"),
    };
    let error = MigrationRunner::run(&conn, &key, &[update(), fail], Some(&path), 11).unwrap_err();
    assert!(matches!(
        error,
        vauchi_core::storage::StorageError::Migration(_)
    ));
    drop(conn);

    let reopened = Connection::open(&path).unwrap();
    assert_eq!(data(&reopened), "kept");
    assert_eq!(MigrationRunner::current_version(&reopened).unwrap(), 1);
    let snapshot = Connection::open_with_flags(
        path.with_extension("pre-migration-v1.bak"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(data(&snapshot), "kept");
    assert_eq!(MigrationRunner::current_version(&snapshot).unwrap(), 1);
}
