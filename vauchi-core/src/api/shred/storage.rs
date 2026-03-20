// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shred Storage Operations
//!
//! Secure file overwrite and widget-level panic shred that operates
//! without full Vauchi initialization.

use std::path::Path;

use crate::api::pre_signed::PreSignedShredMessages;
use crate::storage::secure::SecureStorage;

use super::{SMK_KEY_NAME, ShredError, ShredReport};

/// Public entry point for secure file overwrite, callable from other modules.
pub(crate) fn secure_overwrite_file_public(path: &Path) -> Result<(), std::io::Error> {
    secure_overwrite_file(path)
}

/// Securely overwrites a file with random data then zeros before removing it.
///
/// Uses 2-pass overwrite (#200a): random data (destroys original bit patterns)
/// followed by zeros (verifiable wipe). Both passes are flushed to disk with
/// `sync_all()` to ensure the overwrite reaches physical storage.
pub(super) fn secure_overwrite_file(path: &Path) -> Result<(), std::io::Error> {
    use std::io::{Seek, Write};

    if !path.exists() {
        return Ok(());
    }

    let size = std::fs::metadata(path)?.len() as usize;
    if size == 0 {
        std::fs::remove_file(path)?;
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;

    // Pass 1: Overwrite with random data
    let mut random = vec![0u8; size];
    crate::crypto::random_fill(&mut random);
    file.write_all(&random)?;
    file.sync_all()?;

    // Pass 2: Overwrite with zeros
    file.seek(std::io::SeekFrom::Start(0))?;
    let zeros = vec![0u8; size];
    file.write_all(&zeros)?;
    file.sync_all()?;

    // Close handle, then remove
    drop(file);
    std::fs::remove_file(path)?;
    Ok(())
}

/// Panic shred callable from a widget without full Vauchi initialization.
///
/// This is the core API for iOS/Android home screen widgets that need to
/// trigger a panic shred without opening the full app. It only requires
/// the data directory path and a `SecureStorage` implementation.
///
/// Follows the same 3-phase protocol as `ShredManager::panic_shred()`:
///   1. Load pre-signed messages (if available, before destroying anything)
///   2. Destroy all key material (SMK, identity, key files)
///   3. Delete database, pre-signed file, and data directory
///
/// Network operations (relay purge, contact revocations) are NOT performed
/// by the widget version — the widget has no network access. Pre-signed
/// messages are loaded for future use by the relay cleanup daemon.
pub fn widget_panic_shred(
    data_dir: &Path,
    secure_storage: &dyn SecureStorage,
) -> Result<ShredReport, ShredError> {
    let mut report = ShredReport::default();

    // ── Phase A: Load pre-signed messages while they exist ──
    // We load these before destroying anything, per DP-2 (sign-before-destroy).
    // The widget can't send them (no network), but loading confirms they exist.
    let _pre_signed = PreSignedShredMessages::load(data_dir).ok();

    // ── Phase B: Destroy all key material ──

    // 1. Destroy SMK from SecureStorage
    report.smk_destroyed = secure_storage.secure_delete_key(SMK_KEY_NAME).is_ok();

    // 2. Secure-delete identity backup file
    let identity_path = data_dir.join("identity.json");
    report.identity_file_destroyed = if identity_path.exists() {
        secure_overwrite_file(&identity_path).is_ok()
    } else {
        true // File doesn't exist, nothing to delete
    };

    // 3. Delete all key files
    let keys_dir = data_dir.join("keys");
    report.key_files_destroyed = if keys_dir.exists() {
        let mut count = 0;
        if let Ok(entries) = std::fs::read_dir(&keys_dir) {
            for entry in entries.flatten() {
                if secure_overwrite_file(&entry.path()).is_ok() {
                    count += 1;
                }
            }
        }
        let _ = std::fs::remove_dir(&keys_dir);
        count
    } else {
        0
    };

    // ── Phase C: Cleanup ──

    // 4. Secure-delete SQLite database + WAL/SHM
    let db_path = data_dir.join("vauchi.db");
    let mut db_success = true;
    for suffix in &["", "-wal", "-shm", "-journal"] {
        let path = if suffix.is_empty() {
            db_path.clone()
        } else {
            db_path.with_extension(format!("db{}", suffix))
        };
        if path.exists() && secure_overwrite_file(&path).is_err() {
            db_success = false;
        }
    }
    report.sqlite_destroyed = db_success;

    // 5. Delete pre-signed messages file (secure overwrite, #200a)
    let pre_signed_path = PreSignedShredMessages::file_path(data_dir);
    report.pre_signed_deleted = if pre_signed_path.exists() {
        secure_overwrite_file(&pre_signed_path).is_ok()
    } else {
        true
    };

    // 6. Delete data directory
    report.data_dir_deleted = if data_dir.exists() {
        std::fs::remove_dir_all(data_dir).is_ok()
    } else {
        true
    };

    Ok(report)
}
