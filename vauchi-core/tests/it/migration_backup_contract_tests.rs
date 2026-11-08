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
fn successful_migration_backup_reopens_with_the_previous_data_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let conn = Connection::open(&path).unwrap();
    seed(&conn);
    let key = SymmetricKey::generate();
    MigrationRunner::run(&conn, &key, &[update()], Some(&path), 11).unwrap();
    assert_eq!(data(&conn), "migrated");
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 2);

    let snapshot = Connection::open(path.with_extension("pre-migration-v1.bak")).unwrap();
    assert_eq!(data(&snapshot), "kept");
    assert_eq!(MigrationRunner::current_version(&snapshot).unwrap(), 1);
}

// @internal
#[test]
fn backup_failure_keeps_best_effort_migration_and_emits_only_a_redacted_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private-person-name.db");
    let conn = Connection::open(&path).unwrap();
    seed(&conn);
    let backup = path.with_extension("pre-migration-v1.bak");
    std::fs::write(&backup, b"existing recovery copy").unwrap();
    let diagnostics = Diagnostics::default();
    let key = SymmetricKey::generate();

    tracing::subscriber::with_default(diagnostics.clone(), || {
        MigrationRunner::run(&conn, &key, &[update()], Some(&path), 11).unwrap();
    });

    assert_eq!(data(&conn), "migrated");
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), 2);
    assert_eq!(std::fs::read(&backup).unwrap(), b"existing recovery copy");
    let expected = BTreeMap::from([
        ("level".into(), "WARN".into()),
        ("target".into(), "vauchi.storage.migration".into()),
        ("sqlite_error_code".into(), "1".into()),
        (
            "message".into(),
            "pre-migration backup unavailable; continuing transactionally".into(),
        ),
    ]);
    assert_eq!(*diagnostics.0.lock().unwrap(), vec![expected]);
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
    let snapshot = Connection::open(path.with_extension("pre-migration-v1.bak")).unwrap();
    assert_eq!(data(&snapshot), "kept");
    assert_eq!(MigrationRunner::current_version(&snapshot).unwrap(), 1);
}
