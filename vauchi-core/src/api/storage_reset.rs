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
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && crate::storage::secure_file::confirmed_absent(path) =>
        {
            Ok(())
        }
        Err(error) => Err(remove_error(error)),
    }
}

/// The database's WAL and shared-memory files.
fn companion_files(storage_path: &Path) -> Vec<PathBuf> {
    ["-wal", "-shm", "-journal"]
        .into_iter()
        .map(|suffix| {
            let mut path = storage_path.as_os_str().to_os_string();
            path.push(suffix);
            PathBuf::from(path)
        })
        .collect()
}

/// Deletes only the named database, its companions and managed storage keys.
///
/// Companion files go first: if one cannot be removed, the database stays,
/// so a fresh database never starts next to an old WAL.
/// Retained migration snapshots prevent reset before any data or keys are removed.
/// Use [`delete_unreadable_profile`] with explicit profile authority to remove them.
pub fn delete_unreadable_data(
    storage_path: &Path,
    secure: &dyn SecureStorage,
) -> Result<(), StorageError> {
    crate::storage::migration_backup::ensure_no_ambiguous_backups(storage_path)?;
    for path in companion_files(storage_path) {
        remove_file_if_present(&path)?;
    }
    remove_file_if_present(storage_path)?;
    delete_storage_keys(secure)
}

/// Resets an exclusively owned profile after the caller confirmed starting over.
/// The caller declares ownership of the directory; it must contain the database.
/// Enumeration and target validation precede mutation. A deletion error preserves
/// managed storage keys and is reported even if earlier files were removed.
pub fn delete_unreadable_profile(
    storage_path: &Path,
    profile: &Path,
    secure: &dyn SecureStorage,
) -> Result<(), StorageError> {
    crate::storage::secure_file::validate_profile_root(Some(storage_path), profile)
        .map_err(remove_error)?;
    let snapshots = crate::storage::migration_backup::snapshot_paths(storage_path)?;
    for path in &snapshots {
        let metadata = std::fs::symlink_metadata(path).map_err(remove_error)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(StorageError::InvalidData(
                "unsafe recovery file; reset was not started".into(),
            ));
        }
    }
    for path in companion_files(storage_path) {
        remove_file_if_present(&path)?;
    }
    for path in snapshots {
        remove_file_if_present(&path)?;
    }
    remove_file_if_present(storage_path)?;
    {
        let dir = profile;
        remove_file_if_present(&dir.join("identity.json"))?;
        remove_file_if_present(&legacy_pre_signed_file(dir))?;
        let keys = dir.join("keys");
        if keys.exists() {
            std::fs::remove_dir_all(&keys).map_err(remove_error)?;
        }
    }
    delete_storage_keys(secure)
}

fn delete_storage_keys(secure: &dyn SecureStorage) -> Result<(), StorageError> {
    for name in STORAGE_KEY_NAMES {
        secure.secure_delete_key(name)?;
    }
    secure.secure_delete_key_with_access(BRK_KEY_NAME, KeyAccess::AfterFirstUnlock)?;
    Ok(())
}
