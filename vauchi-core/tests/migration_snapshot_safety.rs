// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(feature = "testing")]

use rusqlite::{Connection, OpenFlags};
use vauchi_core::api::shred::ShredManager;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::migration::{Migration, MigrationAction, MigrationRunner};
use vauchi_core::storage::{FileKeyStorage, MemoryKeyStorage, SecureStorage, Storage};

fn seed(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_version(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
         INSERT INTO schema_version VALUES(1, 5);
         CREATE TABLE recovery_data(value TEXT NOT NULL);
         INSERT INTO recovery_data VALUES('original');",
    )
    .unwrap();
    conn
}

fn migrate(conn: &Connection, path: &std::path::Path) {
    let migration = Migration {
        version: 2,
        name: "fixture update",
        action: MigrationAction::Sql("UPDATE recovery_data SET value='migrated'"),
    };
    MigrationRunner::run(
        conn,
        &SymmetricKey::generate(),
        &[migration],
        Some(path),
        11,
    )
    .unwrap();
    assert_eq!(MigrationRunner::current_version(conn).unwrap(), 2);
}

// @scenario: emergency_shred :: Files are overwritten with zeros before deletion
#[test]
fn snapshot_names_distinguish_same_stem_databases() {
    let root = tempfile::tempdir().unwrap();
    for name in ["shared.db", "shared.sqlite"] {
        let path = root.path().join(name);
        let conn = seed(&path);
        migrate(&conn, &path);
        let snapshot = root.path().join(format!("{name}.pre-migration-v1.bak"));
        let copy =
            Connection::open_with_flags(&snapshot, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(MigrationRunner::current_version(&copy).unwrap(), 1);
        assert_eq!(
            copy.query_row("SELECT value FROM recovery_data", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "original"
        );
    }
}

#[test]
fn preexisting_empty_destinations_are_not_used_as_snapshot_output() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("history.db");
    let conn = seed(&path);
    let candidates = [
        root.path().join("history.pre-migration-v1.bak"),
        root.path().join("history.db.pre-migration-v1.bak"),
    ];
    for candidate in &candidates {
        std::fs::write(candidate, b"").unwrap();
    }
    migrate(&conn, &path);
    for candidate in candidates {
        assert_eq!(std::fs::metadata(candidate).unwrap().len(), 0);
    }
}

#[cfg(unix)]
#[test]
fn snapshot_creation_preserves_preexisting_outside_symlink_targets() {
    for name in [
        "history.pre-migration-v1.bak",
        "history.db.pre-migration-v1.bak",
    ] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("keep");
        std::fs::write(&target, b"").unwrap();
        let path = root.path().join("history.db");
        let conn = seed(&path);
        std::os::unix::fs::symlink(&target, root.path().join(name)).unwrap();
        migrate(&conn, &path);
        assert_eq!(std::fs::metadata(target).unwrap().len(), 0);
    }
}

// @scenario: emergency_shred :: Post-shred verification
#[test]
fn unreadable_real_key_store_is_not_confirmed_absence() {
    let root = tempfile::tempdir().unwrap();
    let keys_dir = root.path().join("keys");
    let writer = FileKeyStorage::new(keys_dir.clone(), SymmetricKey::generate());
    writer.save_key("smk", &[17; 32]).unwrap();
    assert_eq!(writer.load_key("smk").unwrap(), Some(vec![17; 32]));
    let reader = FileKeyStorage::new(keys_dir, SymmetricKey::generate());
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let identity = Identity::create("Fixture", 0);
    let report = ShredManager::new(
        &storage,
        &reader,
        &identity,
        root.path().join("absent-profile"),
    )
    .verify_shred();
    assert_eq!((report.smk_absent, report.all_clear), (false, false));
}

#[cfg(unix)]
#[test]
fn shred_preserves_outside_targets_of_backup_aliases() {
    for hardlink in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("keep");
        std::fs::write(&target, b"outside bytes").unwrap();
        let storage =
            Storage::open(root.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();
        let alias = root.path().join("vauchi.db.pre-migration-v1.bak");
        if hardlink {
            std::fs::hard_link(&target, &alias).unwrap();
        } else {
            std::os::unix::fs::symlink(&target, &alias).unwrap();
        }
        let keys = MemoryKeyStorage::new();
        let identity = Identity::create("Fixture", 0);
        let report = ShredManager::new(&storage, &keys, &identity, root.path())
            .panic_shred()
            .unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"outside bytes");
        assert!(
            !report.sqlite_destroyed,
            "skipped unsafe overwrite must remain visible"
        );
    }
}

#[test]
fn widget_removes_closed_regular_files_and_managed_keys() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("vauchi.db"), b"database fixture").unwrap();
    std::fs::write(root.path().join("identity.json"), b"identity fixture").unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    let report = vauchi_core::api::shred::widget_panic_shred(root.path(), &keys).unwrap();
    assert_eq!(
        (
            report.smk_destroyed,
            report.sqlite_destroyed,
            report.identity_file_destroyed,
            report.data_dir_deleted
        ),
        (true, true, true, true)
    );
    assert_eq!(keys.load_key("smk").unwrap(), None);
    assert!(!root.path().exists());
}

#[test]
fn widget_preserves_an_outside_hardlinked_identity_target() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("keep");
    std::fs::write(&target, b"outside identity fixture").unwrap();
    std::fs::hard_link(&target, root.path().join("identity.json")).unwrap();
    let keys = MemoryKeyStorage::new();
    let report = vauchi_core::api::shred::widget_panic_shred(root.path(), &keys).unwrap();
    assert_eq!(std::fs::read(target).unwrap(), b"outside identity fixture");
    assert_eq!(
        (report.identity_file_destroyed, report.data_dir_deleted),
        (false, true)
    );
}

#[test]
fn real_key_store_does_not_overwrite_an_outside_hardlinked_key() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let writer = FileKeyStorage::new(outside.path().to_path_buf(), key.clone());
    writer.save_key("smk", &[17; 32]).unwrap();
    let source = outside.path().join("smk.key");
    let bytes = std::fs::read(&source).unwrap();
    std::fs::hard_link(&source, root.path().join("smk.key")).unwrap();
    let reader = FileKeyStorage::new(root.path().to_path_buf(), key);
    assert_eq!(reader.load_key("smk").unwrap(), Some(vec![17; 32]));
    let result = reader.secure_delete_key("smk");
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    assert!(matches!(
        result,
        Err(vauchi_core::storage::StorageError::Encryption(_))
    ));
}

#[test]
fn real_key_store_inspection_errors_are_not_absence() {
    let root = tempfile::tempdir().unwrap();
    let not_directory = root.path().join("not-a-directory");
    std::fs::write(&not_directory, b"unreadable key namespace").unwrap();
    let reader = FileKeyStorage::new(not_directory, SymmetricKey::generate());
    let result = reader.load_key("smk");
    assert!(matches!(
        result,
        Err(vauchi_core::storage::StorageError::Encryption(_))
    ));
}
