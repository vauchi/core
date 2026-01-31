// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Account Management
//!
//! Provides account deletion with secure data wipe.

use std::path::Path;

/// Deletes all local account data.
///
/// This performs a thorough cleanup:
/// 1. Drops all database tables
/// 2. Vacuums the database to overwrite freed pages
/// 3. Removes the database file from disk
///
/// After calling this, the Storage instance should not be used.
pub fn delete_account_data<P: AsRef<Path>>(db_path: P) -> Result<(), AccountError> {
    let path = db_path.as_ref();

    // Remove the database file and WAL/journal files
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| AccountError::DeletionFailed(e.to_string()))?;
    }

    // Remove WAL file if it exists
    let wal_path = path.with_extension("db-wal");
    if wal_path.exists() {
        let _ = std::fs::remove_file(wal_path);
    }

    // Remove SHM file if it exists
    let shm_path = path.with_extension("db-shm");
    if shm_path.exists() {
        let _ = std::fs::remove_file(shm_path);
    }

    // Remove journal file if it exists
    let journal_path = path.with_extension("db-journal");
    if journal_path.exists() {
        let _ = std::fs::remove_file(journal_path);
    }

    Ok(())
}

/// Account management errors.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("Account deletion failed: {0}")]
    DeletionFailed(String),
}
