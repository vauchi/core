// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Starting over deletes the unreadable database, the lost identity's files
//! and every key that opened the database, all or nothing for the database
//! (vauchi/private#580, #581).

use std::path::Path;

use vauchi_core::api::PreSignedShredMessages;
use vauchi_core::api::storage_reset::delete_unreadable_data;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage};

fn install(dir: &Path) -> MemoryKeyStorage {
    for name in [
        "vauchi.db",
        "vauchi.db-wal",
        "vauchi.db-shm",
        "vauchi.db.pre-migration-v74.bak",
        "identity.json",
    ] {
        std::fs::write(dir.join(name), b"old").unwrap();
    }
    std::fs::write(PreSignedShredMessages::file_path(dir), b"old purge").unwrap();
    std::fs::create_dir_all(dir.join("keys")).unwrap();
    std::fs::write(dir.join("keys").join("old.key"), b"old").unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[1; 32]).unwrap();
    keys.save_key("storage_bootstrap", &[2; 32]).unwrap();
    keys.save_key("unrelated", &[3; 32]).unwrap();
    keys
}

fn remaining(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// @internal
#[test]
fn starting_over_deletes_the_data_its_companions_and_the_storage_keys() {
    let dir = tempfile::tempdir().unwrap();
    let keys = install(dir.path());

    delete_unreadable_data(&dir.path().join("vauchi.db"), &keys).unwrap();

    assert_eq!(remaining(dir.path()), Vec::<String>::new());
    assert!(!keys.has_key("smk").unwrap());
    assert!(!keys.has_key("storage_bootstrap").unwrap());
    assert!(keys.has_key("unrelated").unwrap(), "only storage keys go");
}

// @internal
#[test]
fn a_companion_that_cannot_be_removed_keeps_the_database_and_the_keys() {
    let dir = tempfile::tempdir().unwrap();
    let keys = install(dir.path());
    std::fs::remove_file(dir.path().join("vauchi.db-wal")).unwrap();
    std::fs::create_dir(dir.path().join("vauchi.db-wal")).unwrap();

    let result = delete_unreadable_data(&dir.path().join("vauchi.db"), &keys);

    assert!(result.is_err());
    assert!(dir.path().join("vauchi.db").exists());
    assert!(keys.has_key("smk").unwrap());
}

// @internal
#[test]
fn starting_over_with_nothing_on_disk_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let keys = MemoryKeyStorage::new();

    delete_unreadable_data(&dir.path().join("vauchi.db"), &keys).unwrap();

    assert_eq!(remaining(dir.path()), Vec::<String>::new());
}
