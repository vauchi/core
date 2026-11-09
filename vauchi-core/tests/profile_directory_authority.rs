// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(feature = "testing")]

use vauchi_core::api::storage_reset::delete_unreadable_data;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage};

#[test]
fn a_database_path_does_not_authorize_emergency_directory_deletion() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = root.path().join("unrelated");
    std::fs::write(&sentinel, b"keep").unwrap();
    let config = VauchiConfig::with_storage_path(root.path().join("custom.db"))
        .with_storage_key(SymmetricKey::generate());
    let mut app = Vauchi::new(config).unwrap();
    app.create_identity("Fixture").unwrap();
    assert!(matches!(
        app.perform_emergency_wipe(true),
        Err(vauchi_core::VauchiError::Configuration(_))
    ));
    assert_eq!(std::fs::read(sentinel).unwrap(), b"keep");
    assert!(app.identity().is_some());
}

#[test]
fn an_explicit_profile_wipe_overwrites_the_actual_database_with_or_without_identity() {
    for has_identity in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut config = VauchiConfig::with_profile_directory(root.path())
            .with_storage_key(SymmetricKey::generate());
        config.storage_path = root.path().join("custom.db");
        let mut app = Vauchi::new(config).unwrap();
        if has_identity {
            app.create_identity("Fixture").unwrap();
        }
        #[cfg(unix)]
        let mut held = std::fs::File::open(root.path().join("custom.db")).unwrap();
        app.perform_emergency_wipe(true).unwrap();
        #[cfg(unix)]
        {
            use std::io::Read;
            let mut header = [0u8; 16];
            held.read_exact(&mut header).unwrap();
            assert_eq!(header, [0; 16]);
        }
        assert!(!root.path().exists());
        assert!(app.identity().is_none());
    }
}

#[test]
fn database_only_reset_preserves_neighboring_identity_and_key_files() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("custom.db");
    std::fs::write(&database, b"unreadable").unwrap();
    std::fs::write(root.path().join("identity.json"), b"neighbor identity").unwrap();
    std::fs::create_dir(root.path().join("keys")).unwrap();
    std::fs::write(root.path().join("keys/neighbor"), b"neighbor key").unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    delete_unreadable_data(&database, &keys).unwrap();
    assert!(!database.exists());
    assert_eq!(keys.load_key("smk").unwrap(), None);
    assert_eq!(
        std::fs::read(root.path().join("identity.json")).unwrap(),
        b"neighbor identity"
    );
    assert_eq!(
        std::fs::read(root.path().join("keys/neighbor")).unwrap(),
        b"neighbor key"
    );
}

#[test]
fn explicit_profile_reset_removes_current_snapshots_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("custom.db");
    let key = SymmetricKey::generate();
    let conn = rusqlite::Connection::open(&database).unwrap();
    conn.execute_batch("CREATE TABLE sentinel(value); INSERT INTO sentinel VALUES (7)")
        .unwrap();
    vauchi_core::storage::migration::MigrationRunner::run(
        &conn,
        &key,
        vauchi_core::storage::migration::all_migrations(),
        Some(&database),
        1,
    )
    .unwrap();
    let snapshot = root.path().join("custom.db.pre-migration-v0.bak");
    assert_eq!(
        rusqlite::Connection::open(&snapshot)
            .unwrap()
            .query_row("SELECT value FROM sentinel", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        7
    );
    drop(conn);
    let storage = vauchi_core::storage::Storage::open(&database, key).unwrap();
    drop(storage);
    assert!(snapshot.exists());
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    vauchi_core::api::storage_reset::delete_unreadable_profile(&database, root.path(), &keys)
        .unwrap();
    assert!(!snapshot.exists());
    assert!(!database.exists());
    assert_eq!(keys.load_key("smk").unwrap(), None);
}

#[test]
fn explicit_profile_reset_rejects_mismatched_scope_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let database = root.path().join("custom.db");
    std::fs::write(&database, b"unreadable").unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    assert!(
        vauchi_core::api::storage_reset::delete_unreadable_profile(&database, other.path(), &keys)
            .is_err()
    );
    assert_eq!(std::fs::read(database).unwrap(), b"unreadable");
    assert_eq!(keys.load_key("smk").unwrap(), Some(vec![17; 32]));
}

#[cfg(unix)]
#[test]
fn partial_wipe_reports_failure_and_retires_the_in_memory_identity() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("keep");
    std::fs::write(&target, b"outside identity").unwrap();
    let mut app = Vauchi::new(
        VauchiConfig::with_profile_directory(root.path())
            .with_storage_key(SymmetricKey::generate()),
    )
    .unwrap();
    app.create_identity("Fixture").unwrap();
    std::os::unix::fs::symlink(&target, root.path().join("identity.json")).unwrap();
    assert!(matches!(
        app.perform_emergency_wipe(true),
        Err(vauchi_core::VauchiError::Configuration(_))
    ));
    assert!(app.identity().is_none());
    assert_eq!(std::fs::read(target).unwrap(), b"outside identity");
}

#[cfg(unix)]
#[test]
fn a_key_directory_alias_does_not_authorize_overwriting_outside_files() {
    for widget in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("keep.key");
        std::fs::write(&target, b"outside key").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("keys")).unwrap();
        let keys = MemoryKeyStorage::new();
        let report = if widget {
            vauchi_core::api::shred::widget_panic_shred(root.path(), &keys).unwrap()
        } else {
            let storage = vauchi_core::storage::Storage::open(
                root.path().join("vauchi.db"),
                SymmetricKey::generate(),
            )
            .unwrap();
            let identity = vauchi_core::identity::Identity::create("Fixture", 0);
            vauchi_core::api::shred::ShredManager::new(&storage, &keys, &identity, root.path())
                .panic_shred()
                .unwrap()
        };
        assert_eq!(std::fs::read(&target).unwrap(), b"outside key");
        assert_eq!(report.key_files_destroyed, 0);
    }
}
