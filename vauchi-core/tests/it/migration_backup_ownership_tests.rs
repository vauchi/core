// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use vauchi_core::api::storage_reset::delete_unreadable_data;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, Storage, StorageError};

fn snapshot(path: &Path, key: &SymmetricKey) -> PathBuf {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE retained_data(value TEXT); INSERT INTO retained_data VALUES ('kept');",
    )
    .unwrap();
    MigrationRunner::run(&conn, key, all_migrations(), Some(path), 1).unwrap();
    let backup = path.with_extension("pre-migration-v0.bak");
    let copy = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        copy.query_row("SELECT value FROM retained_data", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "kept"
    );
    assert_eq!(MigrationRunner::current_version(&copy).unwrap(), 0);
    backup
}

fn keys() -> MemoryKeyStorage {
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    keys.save_key("storage_bootstrap", &[23; 32]).unwrap();
    keys
}

fn assert_keys_retained(keys: &MemoryKeyStorage) {
    assert_eq!(keys.load_key("smk").unwrap(), Some(vec![17; 32]));
    assert_eq!(
        keys.load_key("storage_bootstrap").unwrap(),
        Some(vec![23; 32])
    );
}

// @internal
#[test]
fn startup_preserves_actual_foreign_and_same_stem_recovery_copies() {
    for current_name in ["current.db", "other.sqlite"] {
        let dir = tempfile::tempdir().unwrap();
        let key = SymmetricKey::generate();
        let backup = snapshot(&dir.path().join("other.db"), &key);
        let bytes = std::fs::read(&backup).unwrap();

        let storage = Storage::open(dir.path().join(current_name), key).unwrap();

        assert_eq!(
            storage.schema_version().unwrap(),
            all_migrations().last().unwrap().version
        );
        assert_eq!(std::fs::read(&backup).unwrap(), bytes);
    }
}

// @internal
#[test]
fn startup_retains_a_preexisting_own_snapshot_when_no_migration_is_pending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let key = SymmetricKey::generate();
    let backup = snapshot(&path, &key);
    let bytes = std::fs::read(&backup).unwrap();

    let storage = Storage::open(&path, key).unwrap();

    assert_eq!(
        storage.schema_version().unwrap(),
        all_migrations().last().unwrap().version
    );
    assert_eq!(std::fs::read(&backup).unwrap(), bytes);
}

// @internal
#[test]
fn startup_cleans_a_snapshot_created_by_its_own_successful_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");

    let storage = Storage::open(&path, SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage.schema_version().unwrap(),
        all_migrations().last().unwrap().version
    );
    assert!(!path.with_extension("pre-migration-v0.bak").exists());
}

// @internal
#[test]
fn startup_does_not_claim_an_empty_preexisting_destination_as_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let backup = path.with_extension("pre-migration-v0.bak");
    std::fs::write(&backup, b"").unwrap();

    let storage = Storage::open(&path, SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage.schema_version().unwrap(),
        all_migrations().last().unwrap().version
    );
    let copy = Connection::open_with_flags(backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(MigrationRunner::current_version(&copy).unwrap(), 0);
}

#[cfg(unix)]
// @internal
#[test]
fn cleanup_failure_after_commit_does_not_prevent_opening_a_readable_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    // #1178: SQLite resolves file URIs; remove_file sees a literal path and returns NotFound.
    let uri = format!("file:{}", path.display());

    let result = Storage::open(&uri, SymmetricKey::generate());

    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let expected = all_migrations().last().unwrap().version;
    assert_eq!(MigrationRunner::current_version(&conn).unwrap(), expected);
    let recovery = Connection::open_with_flags(
        path.with_extension("pre-migration-v0.bak"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(MigrationRunner::current_version(&recovery).unwrap(), 0);
    let storage = result.unwrap();
    assert_eq!(storage.schema_version().unwrap(), expected);
}

// @internal
#[test]
fn reset_refuses_an_actual_legacy_snapshot_before_removing_data_or_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private-person.db");
    let backup = snapshot(&path, &SymmetricKey::generate());
    let database_bytes = std::fs::read(&path).unwrap();
    let backup_bytes = std::fs::read(&backup).unwrap();
    let wal = dir.path().join("private-person.db-wal");
    std::fs::write(&wal, b"pending recovery").unwrap();
    let keys = keys();

    let error = delete_unreadable_data(&path, &keys).unwrap_err();

    assert!(matches!(error, StorageError::InvalidData(_)));
    assert_eq!(
        error.to_string(),
        "Invalid data: migration backup ownership is ambiguous; recovery data was preserved"
    );
    assert_eq!(std::fs::read(&path).unwrap(), database_bytes);
    assert_eq!(std::fs::read(&backup).unwrap(), backup_bytes);
    assert_eq!(std::fs::read(&wal).unwrap(), b"pending recovery");
    assert_keys_retained(&keys);
}

// @internal
#[test]
fn reset_preserves_a_same_stem_databases_snapshot_and_its_own_data() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let backup = snapshot(&dir.path().join("shared.sqlite"), &key);
    let backup_bytes = std::fs::read(&backup).unwrap();
    let path = dir.path().join("shared.db");
    std::fs::write(&path, b"unreadable own data").unwrap();
    let keys = keys();

    let error = delete_unreadable_data(&path, &keys).unwrap_err();

    assert!(matches!(error, StorageError::InvalidData(_)));
    assert_eq!(std::fs::read(&path).unwrap(), b"unreadable own data");
    assert_eq!(std::fs::read(&backup).unwrap(), backup_bytes);
    assert_keys_retained(&keys);
}

// @internal
#[test]
fn reset_preserves_unrelated_copies_and_lookalikes_while_deleting_owned_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    std::fs::write(&path, b"unreadable").unwrap();
    let names = [
        "other.pre-migration-v1.bak",
        "history.pre-migration-vx.bak",
        "history.pre-migration-v1.bak.extra",
        "history.db.pre-migration-not-a-backup",
    ];
    for name in names {
        std::fs::write(dir.path().join(name), name.as_bytes()).unwrap();
    }
    let keys = keys();

    delete_unreadable_data(&path, &keys).unwrap();

    assert!(!path.exists());
    assert_eq!(keys.load_key("smk").unwrap(), None);
    for name in names {
        assert_eq!(
            std::fs::read(dir.path().join(name)).unwrap(),
            name.as_bytes()
        );
    }
}

// @internal
#[test]
fn reset_refuses_an_ambiguous_directory_without_removing_any_owned_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    std::fs::write(&path, b"unreadable").unwrap();
    let candidate = dir.path().join("history.pre-migration-v4.bak");
    std::fs::create_dir(&candidate).unwrap();
    let keys = keys();

    let error = delete_unreadable_data(&path, &keys).unwrap_err();

    assert!(matches!(error, StorageError::InvalidData(_)));
    assert_eq!(std::fs::read(&path).unwrap(), b"unreadable");
    assert!(candidate.is_dir());
    assert_keys_retained(&keys);
}

// @internal
#[test]
fn reset_does_not_turn_failed_enumeration_into_successful_key_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("private-parent");
    std::fs::write(&parent, b"not a directory").unwrap();
    let keys = keys();

    let error = delete_unreadable_data(&parent.join("history.db"), &keys).unwrap_err();

    assert!(matches!(error, StorageError::InvalidData(_)));
    assert!(!error.to_string().contains("private-parent"));
    assert_eq!(std::fs::read(parent).unwrap(), b"not a directory");
    assert_keys_retained(&keys);
}

#[cfg(unix)]
// @internal
#[test]
fn reset_refuses_ambiguous_symlinks_without_touching_external_targets() {
    for dangling in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let target = external.path().join("keep");
        if !dangling {
            std::fs::write(&target, b"external recovery").unwrap();
        }
        let path = dir.path().join("history.db");
        std::fs::write(&path, b"unreadable").unwrap();
        let candidate = dir.path().join("history.pre-migration-v4.bak");
        std::os::unix::fs::symlink(&target, &candidate).unwrap();
        let keys = keys();

        let error = delete_unreadable_data(&path, &keys).unwrap_err();

        assert!(matches!(error, StorageError::InvalidData(_)));
        assert_eq!(std::fs::read(&path).unwrap(), b"unreadable");
        assert!(
            std::fs::symlink_metadata(candidate)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        if dangling {
            assert!(!target.exists());
        } else {
            assert_eq!(std::fs::read(target).unwrap(), b"external recovery");
        }
        assert_keys_retained(&keys);
    }
}
