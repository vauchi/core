// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(feature = "testing")]

use vauchi_core::api::shred::{ShredError, ShredManager};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, Storage};

// @internal
#[test]
fn an_unrelated_root_is_rejected_before_keys_or_files_are_changed() {
    let database = tempfile::tempdir().unwrap();
    let unrelated = tempfile::tempdir().unwrap();
    let sentinel = unrelated.path().join("keep");
    std::fs::write(&sentinel, b"unrelated profile").unwrap();
    let path = database.path().join("vauchi.db");
    let storage = Storage::open(&path, SymmetricKey::generate()).unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    let identity = Identity::create("Fixture", 0);
    let error = ShredManager::new(&storage, &keys, &identity, unrelated.path())
        .panic_shred()
        .unwrap_err();
    match error {
        ShredError::FileError(detail) => assert_eq!(
            detail,
            "profile directory is not associated with the database"
        ),
        other => panic!("unexpected scope error: {other}"),
    }
    assert_eq!(std::fs::read(sentinel).unwrap(), b"unrelated profile");
    assert_eq!(keys.load_key("smk").unwrap(), Some(vec![17; 32]));
    assert!(path.exists());
}

// @internal
#[test]
fn widget_rejects_a_non_directory_before_destroying_keys() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("not-a-profile");
    std::fs::write(&file, b"unrelated file").unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    let result = vauchi_core::api::shred::widget_panic_shred(&file, &keys);
    assert!(matches!(result, Err(ShredError::FileError(_))));
    assert_eq!(std::fs::read(file).unwrap(), b"unrelated file");
    assert_eq!(keys.load_key("smk").unwrap(), Some(vec![17; 32]));
}

#[cfg(unix)]
// @internal
#[test]
fn custom_database_overwrite_report_matches_the_actual_file() {
    use std::io::Read;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("custom.db");
    let storage = Storage::open(&path, SymmetricKey::generate()).unwrap();
    storage.wal_checkpoint().unwrap();
    let mut held = std::fs::File::open(&path).unwrap();
    let keys = MemoryKeyStorage::new();
    let identity = Identity::create("Fixture", 0);
    let report = ShredManager::new(&storage, &keys, &identity, root.path())
        .panic_shred()
        .unwrap();
    let mut header = [0u8; 16];
    held.read_exact(&mut header).unwrap();
    assert_eq!(header, [0; 16]);
    assert!(report.sqlite_destroyed);
    assert!(!path.exists());
}
