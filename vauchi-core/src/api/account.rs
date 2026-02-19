// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Account Management
#![allow(dead_code)]
//!
//! Provides account deletion with secure data wipe and GDPR deletion grace period.

use std::path::Path;

use crate::identity::Identity;
use crate::network::AccountRevoked;
use crate::storage::{DeletionState, Storage, StorageError};

/// Duration of deletion grace period in seconds (7 days).
const DELETION_GRACE_PERIOD_SECS: u64 = 7 * 24 * 60 * 60;

/// Deletes all local account data with secure overwrite.
///
/// Uses `secure_overwrite_file` to overwrite database files with random data
/// and zeros before unlinking, preventing forensic recovery (#200a).
///
/// After calling this, the Storage instance should not be used.
pub fn delete_account_data<P: AsRef<Path>>(db_path: P) -> Result<(), AccountError> {
    let path = db_path.as_ref();

    // Securely overwrite the database file and WAL/journal files
    for suffix in &["", "-wal", "-shm", "-journal"] {
        let file_path = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            path.with_extension(format!("db{}", suffix))
        };
        if file_path.exists() {
            super::shred::secure_overwrite_file_public(&file_path)
                .map_err(|e| AccountError::DeletionFailed(e.to_string()))?;
        }
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

/// Result from executing account deletion.
///
/// Contains the revocation messages that must be sent to contacts via relay.
/// The caller is responsible for relay delivery and subsequent database file deletion.
#[derive(Debug, Clone)]
pub struct DeletionResult {
    /// `AccountRevoked` messages to send to contacts via relay.
    pub revocations: Vec<AccountRevoked>,
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
    ///
    /// This performs the GDPR-compliant deletion protocol:
    /// 1. Verifies the grace period has elapsed
    /// 2. Loads all contacts
    /// 3. For each contact: generates an `AccountRevoked` message and shreds the CEK
    /// 4. Marks the deletion state as `Executed`
    ///
    /// Returns a `DeletionResult` containing the revocation messages that the caller
    /// must send to contacts via the relay. After sending, the caller should invoke
    /// `delete_account_data()` to remove the database files.
    ///
    /// CEKs are destroyed before the state is marked as Executed, so even if the
    /// process is interrupted, card data is already unreadable.
    pub fn execute_deletion(&self, identity: &Identity) -> Result<DeletionResult, AccountError> {
        let current = self.storage.load_deletion_state()?;
        match current {
            DeletionState::Scheduled { execute_at, .. } => {
                let now = current_timestamp();
                if now < execute_at {
                    return Err(AccountError::GracePeriodNotElapsed);
                }

                // Load all contacts
                let contacts = self
                    .storage
                    .list_contacts()
                    .map_err(|e| AccountError::DeletionFailed(e.to_string()))?;

                let mut revocations = Vec::with_capacity(contacts.len());

                for contact in &contacts {
                    // Generate signed AccountRevoked message
                    let revoked = AccountRevoked::create(identity, contact.id(), now);
                    revocations.push(revoked);

                    // Crypto-shred: delete CEK (card becomes permanently unreadable)
                    // This runs BEFORE state is marked Executed for crash safety
                    self.storage
                        .delete_contact_cek(contact.id())
                        .map_err(|e| AccountError::DeletionFailed(e.to_string()))?;

                    // Delete contact row and ratchet state (#48)
                    // After CEK shredding, rows contain metadata (public_key,
                    // display_name, timestamps) that should not survive deletion.
                    self.storage
                        .delete_contact(contact.id())
                        .map_err(|e| AccountError::DeletionFailed(e.to_string()))?;
                }

                // Flush WAL so secure_delete=ON takes effect on deleted data (#81)
                self.storage
                    .wal_checkpoint()
                    .map_err(|e| AccountError::DeletionFailed(e.to_string()))?;

                // Mark state as executed
                let state = DeletionState::Executed { executed_at: now };
                self.storage.save_deletion_state(&state)?;

                Ok(DeletionResult { revocations })
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
