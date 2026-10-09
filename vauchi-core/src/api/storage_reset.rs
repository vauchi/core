// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deleting data no key opens, after the person confirmed starting over
//! (vauchi/private#580, #581). Shared by the mobile and desktop bindings.

use std::path::{Path, PathBuf};

use crate::api::PreSignedShredMessages;
use crate::storage::{SecureStorage, StorageError};

/// Every secure-storage name that holds a key opening the database.
const STORAGE_KEY_NAMES: [&str; 2] = ["smk", "storage_bootstrap"];

fn remove_error(error: std::io::Error) -> StorageError {
    StorageError::InvalidData(format!("could not remove a storage file: {}", error.kind()))
}

fn remove_file_if_present(path: &Path) -> Result<(), StorageError> {
    if path.exists() {
        std::fs::remove_file(path).map_err(remove_error)?;
    }
    Ok(())
}

/// The database's WAL and shared-memory files and pre-migration backups.
fn companion_files(storage_path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Some(name) = storage_path.file_name().and_then(|n| n.to_str()) else {
        return files;
    };
    let Some(dir) = storage_path.parent() else {
        return files;
    };
    files.push(dir.join(format!("{name}-wal")));
    files.push(dir.join(format!("{name}-shm")));
    if let Ok(entries) = std::fs::read_dir(dir) {
        let backup_prefix = format!("{name}.pre-migration-");
        files.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&backup_prefix))
        }));
    }
    files
}

/// Deletes the database, the lost identity's files and every key that
/// opened the database.
///
/// Companion files go first: if one cannot be removed, the database stays,
/// so a fresh database never starts next to an old WAL.
pub fn delete_unreadable_data(
    storage_path: &Path,
    secure: &dyn SecureStorage,
) -> Result<(), StorageError> {
    for path in companion_files(storage_path) {
        remove_file_if_present(&path)?;
    }
    remove_file_if_present(storage_path)?;
    if let Some(dir) = storage_path.parent() {
        remove_file_if_present(&dir.join("identity.json"))?;
        remove_file_if_present(&PreSignedShredMessages::file_path(dir))?;
        let keys = dir.join("keys");
        if keys.exists() {
            std::fs::remove_dir_all(&keys).map_err(remove_error)?;
        }
    }
    for name in STORAGE_KEY_NAMES {
        secure.secure_delete_key(name)?;
    }
    Ok(())
}
