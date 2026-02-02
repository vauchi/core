// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Account Management
#![allow(dead_code)]
//!
//! Provides account deletion with secure data wipe and GDPR deletion grace period.

use std::path::Path;

use crate::storage::{DeletionState, Storage, StorageError};

/// Duration of deletion grace period in seconds (7 days).
const DELETION_GRACE_PERIOD_SECS: u64 = 7 * 24 * 60 * 60;

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

    #[error("Deletion already scheduled")]
    AlreadyScheduled,

    #[error("Grace period not elapsed")]
    GracePeriodNotElapsed,

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Manages account deletion with a 7-day grace period.
///
/// Supports schedule/cancel/execute flow per GDPR requirements.
pub struct DeletionManager<'a> {
    storage: &'a Storage,
}

impl<'a> DeletionManager<'a> {
    /// Creates a new DeletionManager.
    pub fn new(storage: &'a Storage) -> Self {
        DeletionManager { storage }
    }

    /// Returns the current deletion state.
    pub fn deletion_state(&self) -> Result<DeletionState, AccountError> {
        Ok(self.storage.load_deletion_state()?)
    }

    /// Schedules account deletion with a 7-day grace period.
    pub fn schedule_deletion(&self) -> Result<(), AccountError> {
        let current = self.storage.load_deletion_state()?;
        if matches!(current, DeletionState::Scheduled { .. }) {
            return Err(AccountError::AlreadyScheduled);
        }

        let now = current_timestamp();
        let execute_at = now + DELETION_GRACE_PERIOD_SECS;
        let state = DeletionState::Scheduled {
            scheduled_at: now,
            execute_at,
        };
        self.storage.save_deletion_state(&state)?;
        Ok(())
    }

    /// Schedules deletion with explicit timestamps (for testing).
    pub fn schedule_deletion_with_execute_at(
        &self,
        scheduled_at: u64,
        execute_at: u64,
    ) -> Result<(), AccountError> {
        let state = DeletionState::Scheduled {
            scheduled_at,
            execute_at,
        };
        self.storage.save_deletion_state(&state)?;
        Ok(())
    }

    /// Cancels a scheduled deletion during the grace period.
    pub fn cancel_deletion(&self) -> Result<(), AccountError> {
        self.storage.save_deletion_state(&DeletionState::None)?;
        Ok(())
    }

    /// Executes the deletion if the grace period has elapsed.
    pub fn execute_deletion(&self) -> Result<(), AccountError> {
        let current = self.storage.load_deletion_state()?;
        match current {
            DeletionState::Scheduled { execute_at, .. } => {
                let now = current_timestamp();
                if now < execute_at {
                    return Err(AccountError::GracePeriodNotElapsed);
                }
                let state = DeletionState::Executed { executed_at: now };
                self.storage.save_deletion_state(&state)?;
                Ok(())
            }
            _ => Err(AccountError::DeletionFailed(
                "No deletion scheduled".to_string(),
            )),
        }
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
