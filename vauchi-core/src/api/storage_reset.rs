// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Deleting data no key opens, after the person confirmed starting over
//! (vauchi/private#580, #581). Shared by the mobile and desktop bindings.

use std::path::{Path, PathBuf};

use crate::api::legacy_pre_signed_file;
use crate::api::shred::BRK_KEY_NAME;
use crate::storage::{KeyAccess, SecureStorage, StorageError};

/// Every secure-storage name that holds a key opening the database.
const STORAGE_KEY_NAMES: [&str; 2] = ["smk", "storage_bootstrap"];

fn remove_error(error: std::io::Error) -> StorageError {
    StorageError::InvalidData(format!("could not remove a storage file: {}", error.kind()))
}

fn remove_file_if_present(path: &Path) -> Result<(), StorageError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(remove_error(error)),
    }
}

/// The database's WAL and shared-memory files.
fn companion_files(storage_path: &Path) -> Vec<PathBuf> {
    ["-wal", "-shm"]
        .into_iter()
        .map(|suffix| {
            let mut path = storage_path.as_os_str().to_os_string();
            path.push(suffix);
            PathBuf::from(path)
        })
        .collect()
}

/// Deletes the database, the lost identity's files and every key that
/// opened the database.
///
/// Companion files go first: if one cannot be removed, the database stays,
/// so a fresh database never starts next to an old WAL.
/// Ambiguous legacy migration snapshots prevent reset before any data or keys
/// are removed; their filename alone cannot establish database ownership.
pub fn delete_unreadable_data(
    storage_path: &Path,
    secure: &dyn SecureStorage,
) -> Result<(), StorageError> {
    crate::storage::migration_backup::ensure_no_ambiguous_backups(storage_path)?;
    for path in companion_files(storage_path) {
        remove_file_if_present(&path)?;
    }
    remove_file_if_present(storage_path)?;
    if let Some(dir) = storage_path.parent() {
        remove_file_if_present(&dir.join("identity.json"))?;
        remove_file_if_present(&legacy_pre_signed_file(dir))?;
        let keys = dir.join("keys");
        if keys.exists() {
            std::fs::remove_dir_all(&keys).map_err(remove_error)?;
        }
    }
    for name in STORAGE_KEY_NAMES {
        secure.secure_delete_key(name)?;
    }
    secure.secure_delete_key_with_access(BRK_KEY_NAME, KeyAccess::AfterFirstUnlock)?;
    Ok(())
}
