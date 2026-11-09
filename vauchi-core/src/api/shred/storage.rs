// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shred Storage Operations
//!
//! Secure file overwrite and widget-level panic shred that operates
//! without full Vauchi initialization.

use std::path::Path;

use crate::api::pre_signed::legacy_pre_signed_file;
use crate::storage::secure::{KeyAccess, SecureStorage};
use crate::storage::secure_file::{confirmed_absent, validate_profile_root};

use super::{BOOTSTRAP_KEY_NAME, BRK_KEY_NAME, SMK_KEY_NAME, ShredError, ShredReport};
pub(super) use crate::storage::secure_file::secure_overwrite_file;

/// Public entry point for secure file overwrite, callable from other modules.
pub(crate) fn secure_overwrite_file_public(path: &Path) -> Result<(), std::io::Error> {
    secure_overwrite_file(path)
}

/// Deletes every key in secure storage that opens stored data, the BRK
/// with the SMK (ADR-033 amendment 2026-11-07); true when every delete
/// succeeded.
pub(super) fn destroy_storage_keys(secure_storage: &dyn SecureStorage) -> bool {
    let smk = secure_storage.secure_delete_key(SMK_KEY_NAME).is_ok();
    let bootstrap = secure_storage.secure_delete_key(BOOTSTRAP_KEY_NAME).is_ok();
    let brk = secure_storage
        .secure_delete_key_with_access(BRK_KEY_NAME, KeyAccess::AfterFirstUnlock)
        .is_ok();
    smk && bootstrap && brk
}

pub(super) fn secure_delete_database_files(database: &Path) -> bool {
    let mut complete = true;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = database.as_os_str().to_os_string();
        name.push(suffix);
        complete &= secure_overwrite_file(Path::new(&name)).is_ok();
    }
    match crate::storage::migration_backup::snapshot_paths(database) {
        Ok(paths) => {
            for path in paths {
                complete &= secure_overwrite_file(&path).is_ok();
            }
        }
        Err(_) => complete = false,
    }
    complete
}

pub(super) fn delete_key_files(directory: &Path) -> (usize, bool) {
    let metadata = match std::fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(_) => return (0, confirmed_absent(directory)),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return (0, false);
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return (0, false);
    };
    let mut count = 0;
    let mut complete = true;
    for entry in entries {
        match entry {
            Ok(entry) if secure_overwrite_file(&entry.path()).is_ok() => count += 1,
            _ => complete = false,
        }
    }
    complete &= std::fs::remove_dir(directory).is_ok();
    (count, complete)
}

/// Panic shred callable from a widget without full Vauchi initialization.
///
/// This is the core API for iOS/Android home screen widgets that need to
/// trigger a panic shred without opening the full app. It only requires
/// the data directory path and a `SecureStorage` implementation.
///
/// Destroys what `ShredManager::panic_shred()` destroys, in the same order:
/// key material (SMK, identity, key files) first, then the database, a
/// pre-signed file an older install left, and the data directory. Like
/// every emergency wipe it sends nothing (vauchi/private#910).
pub fn widget_panic_shred(
    data_dir: &Path,
    secure_storage: &dyn SecureStorage,
) -> Result<ShredReport, ShredError> {
    shred_profile_database(data_dir, &data_dir.join("vauchi.db"), secure_storage)
}

pub(crate) fn shred_profile_database(
    data_dir: &Path,
    database: &Path,
    secure_storage: &dyn SecureStorage,
) -> Result<ShredReport, ShredError> {
    validate_profile_root(Some(database), data_dir).map_err(|_| {
        ShredError::FileError("profile directory is not associated with the database".into())
    })?;
    // ── Phase A: Destroy all key material ──

    // 1. Destroy SMK from SecureStorage
    let mut report = ShredReport {
        smk_destroyed: destroy_storage_keys(secure_storage),
        ..ShredReport::default()
    };

    // 2. Secure-delete identity backup file
    let identity_path = data_dir.join("identity.json");
    report.identity_file_destroyed = secure_overwrite_file(&identity_path).is_ok();

    // 3. Delete all key files
    let keys_dir = data_dir.join("keys");
    (report.key_files_destroyed, report.key_files_complete) = delete_key_files(&keys_dir);

    // ── Phase B: Cleanup ──

    // 4. Secure-delete SQLite database + WAL/SHM
    report.sqlite_destroyed = secure_delete_database_files(database);

    // 5. Delete pre-signed messages file (secure overwrite, #200a)
    let pre_signed_path = legacy_pre_signed_file(data_dir);
    report.pre_signed_deleted = secure_overwrite_file(&pre_signed_path).is_ok();

    // 6. Delete data directory
    report.data_dir_deleted = if data_dir.exists() {
        std::fs::remove_dir_all(data_dir).is_ok()
    } else {
        confirmed_absent(data_dir)
    };

    Ok(report)
}
