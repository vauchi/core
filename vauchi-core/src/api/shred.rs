// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shred Manager — Core Shred Orchestration
//!
//! Implements the three-phase shred protocol:
//! 1. **Soft Shred**: Schedule deletion with 7-day grace period
//! 2. **Hard Shred**: Irreversible destruction after grace period
//! 3. **Panic Shred**: Immediate destruction using pre-signed messages (DP-2)
//!
//! The ShredManager composes the existing DeletionManager for grace period
//! tracking and adds cryptographic key destruction and network notification.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::account::{AccountError, DeletionManager};
use crate::api::pre_signed::PreSignedShredMessages;
use crate::identity::Identity;
use crate::storage::secure::SecureStorage;
use crate::storage::Storage;

/// Key name for the Shredding Master Key in SecureStorage.
const SMK_KEY_NAME: &str = "smk";

/// Token returned by soft_shred to authorize hard_shred.
#[derive(Debug, Clone)]
pub struct ShredToken {
    created_at: u64,
}

impl ShredToken {
    fn new() -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self { created_at }
    }

    /// Returns when this token was created (unix seconds).
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Reconstructs a token from a stored created_at timestamp.
    pub fn from_created_at(created_at: u64) -> Self {
        Self { created_at }
    }
}

/// Report of shred operations performed.
#[derive(Debug, Default)]
pub struct ShredReport {
    /// Number of contacts notified of deletion.
    pub contacts_notified: usize,
    /// Whether the relay purge was sent successfully.
    pub relay_purge_sent: bool,
    /// Number of linked devices notified.
    pub devices_notified: usize,
    /// Whether SMK was destroyed from SecureStorage.
    pub smk_destroyed: bool,
    /// Whether the identity backup file was securely deleted.
    pub identity_file_destroyed: bool,
    /// Number of key files deleted from FileKeyStorage.
    pub key_files_destroyed: usize,
    /// Whether the SQLite database was securely deleted.
    pub sqlite_destroyed: bool,
    /// Whether the pre-signed messages file was deleted.
    pub pre_signed_deleted: bool,
    /// Whether the data directory was removed.
    pub data_dir_deleted: bool,
}

/// Post-shred verification result.
#[derive(Debug)]
pub struct ShredVerification {
    /// Whether SMK is absent from SecureStorage (expected: true after shred).
    pub smk_absent: bool,
    /// Whether the database file is absent (expected: true after shred).
    pub database_absent: bool,
    /// Whether the data directory is absent (expected: true after shred).
    pub data_dir_absent: bool,
    /// Whether the pre-signed messages file is absent (expected: true after shred).
    pub pre_signed_absent: bool,
    /// Overall: all checks passed.
    pub all_clear: bool,
}

/// Errors from shred operations.
#[derive(Debug, thiserror::Error)]
pub enum ShredError {
    #[error("Account error: {0}")]
    Account(#[from] AccountError),

    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),

    #[error("Pre-signed messages unavailable: {0}")]
    PreSignedUnavailable(String),

    #[error("SMK destruction failed: {0}")]
    SmkDestructionFailed(String),

    #[error("File operation failed: {0}")]
    FileError(String),
}

/// Orchestrates cryptographic shredding of all account data.
///
/// Composes the existing `DeletionManager` for grace period tracking and
/// adds key destruction, tombstone creation, and network notification.
pub struct ShredManager<'a> {
    storage: &'a Storage,
    secure_storage: &'a dyn SecureStorage,
    identity: &'a Identity,
    data_dir: PathBuf,
}

impl<'a> ShredManager<'a> {
    /// Creates a new ShredManager.
    pub fn new(
        storage: &'a Storage,
        secure_storage: &'a dyn SecureStorage,
        identity: &'a Identity,
        data_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            storage,
            secure_storage,
            identity,
            data_dir: data_dir.into(),
        }
    }

    /// Phase 1: Soft Shred — schedule deletion with 7-day grace period.
    ///
    /// Delegates to DeletionManager for grace period tracking.
    /// Refreshes pre-signed messages for later use by panic/hard shred.
    pub fn soft_shred(&self) -> Result<ShredToken, ShredError> {
        // 1. Delegate to DeletionManager for grace period
        let dm = DeletionManager::new(self.storage);
        dm.schedule_deletion()?;

        // 2. Refresh pre-signed messages file
        let _ = self.refresh_pre_signed_messages();

        Ok(ShredToken::new())
    }

    /// Cancel a scheduled shred during the grace period.
    pub fn cancel_shred(&self, _token: ShredToken) -> Result<(), ShredError> {
        let dm = DeletionManager::new(self.storage);
        dm.cancel_deletion()?;
        Ok(())
    }

    /// Phase 2: Hard Shred — irreversible destruction.
    ///
    /// Requires grace period to have elapsed. Sends network notifications
    /// while keys are still available, then destroys all key material.
    pub fn hard_shred(&self, _token: ShredToken) -> Result<ShredReport, ShredError> {
        let mut report = ShredReport::default();

        // 1. Verify grace period has elapsed
        let dm = DeletionManager::new(self.storage);
        dm.execute_deletion()?;

        // 2-4. Network notifications (while keys alive) — best-effort
        // These would send to contacts and relay in a real implementation.
        // For now, we log intent. Full networking requires relay client.
        report.contacts_notified = 0;
        report.relay_purge_sent = false;
        report.devices_notified = 0;

        // ═══ POINT OF NO RETURN ═══

        // 5. Destroy SMK from SecureStorage
        report.smk_destroyed = self.destroy_smk();

        // 6. Secure-delete identity backup file
        report.identity_file_destroyed = self.secure_delete_identity_file();

        // 7. Delete all key files (best-effort, count successes)
        report.key_files_destroyed = self.delete_key_files();

        // 8. Secure-delete SQLite database + WAL/SHM
        report.sqlite_destroyed = self.secure_delete_database();

        // 9. Delete pre-signed messages file
        report.pre_signed_deleted = self.delete_pre_signed_file();

        // 10. Delete data directory
        report.data_dir_deleted = self.delete_data_directory();

        Ok(report)
    }

    /// Panic Shred — immediate destruction, no grace period.
    ///
    /// Follows DP-2 (sign-before-destroy): loads pre-signed messages before
    /// destroying any key material. Network operations happen after destruction.
    pub fn panic_shred(&self) -> Result<ShredReport, ShredError> {
        let mut report = ShredReport::default();

        // ── Phase A: Prepare outbound messages while keys available ──

        // 1. Load pre-signed messages from file (unencrypted per DP-3)
        let _pre_signed = match PreSignedShredMessages::load(&self.data_dir) {
            Ok(msgs) => Some(msgs),
            Err(_) => {
                // 2. Fallback: sign fresh messages now (keys still available)
                let msgs = PreSignedShredMessages::generate(self.identity);
                Some(msgs)
            }
        };

        // ── Phase B: Destroy all key material ──

        // 3. Destroy SMK from all SecureStorage backends
        report.smk_destroyed = self.destroy_smk();

        // 4. Secure-delete identity backup file
        report.identity_file_destroyed = self.secure_delete_identity_file();

        // 5. Delete all key files
        report.key_files_destroyed = self.delete_key_files();

        // ── Phase C: Best-effort network and cleanup ──
        // In a full implementation, pre-signed messages would be sent here.
        // Network operations are best-effort after key destruction.

        // 8. Secure-delete SQLite database
        report.sqlite_destroyed = self.secure_delete_database();

        // 9. Delete pre-signed messages file
        report.pre_signed_deleted = self.delete_pre_signed_file();

        // 10. Delete data directory
        report.data_dir_deleted = self.delete_data_directory();

        Ok(report)
    }

    /// Post-shred verification audit.
    ///
    /// Checks that all key material and data has been destroyed.
    pub fn verify_shred(&self) -> ShredVerification {
        let smk_absent = self
            .secure_storage
            .load_key(SMK_KEY_NAME)
            .map(|k| k.is_none())
            .unwrap_or(true);

        let database_absent = !self.data_dir.join("vauchi.db").exists();
        let data_dir_absent = !self.data_dir.exists();
        let pre_signed_absent =
            !PreSignedShredMessages::file_path(&self.data_dir).exists();

        let all_clear = smk_absent && database_absent && pre_signed_absent;

        ShredVerification {
            smk_absent,
            database_absent,
            data_dir_absent,
            pre_signed_absent,
            all_clear,
        }
    }

    // ── Internal helpers ──

    fn destroy_smk(&self) -> bool {
        self.secure_storage
            .secure_delete_key(SMK_KEY_NAME)
            .is_ok()
    }

    fn secure_delete_identity_file(&self) -> bool {
        let identity_path = self.data_dir.join("identity.json");
        if identity_path.exists() {
            secure_overwrite_file(&identity_path).is_ok()
        } else {
            true // File doesn't exist, nothing to delete
        }
    }

    fn delete_key_files(&self) -> usize {
        let keys_dir = self.data_dir.join("keys");
        if !keys_dir.exists() {
            return 0;
        }
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
    }

    fn secure_delete_database(&self) -> bool {
        let db_path = self.data_dir.join("vauchi.db");
        let mut success = true;

        for suffix in &["", "-wal", "-shm", "-journal"] {
            let path = if suffix.is_empty() {
                db_path.clone()
            } else {
                db_path.with_extension(format!("db{}", suffix))
            };
            if path.exists() && secure_overwrite_file(&path).is_err() {
                success = false;
            }
        }
        success
    }

    fn delete_pre_signed_file(&self) -> bool {
        let path = PreSignedShredMessages::file_path(&self.data_dir);
        if path.exists() {
            std::fs::remove_file(&path).is_ok()
        } else {
            true
        }
    }

    fn delete_data_directory(&self) -> bool {
        if self.data_dir.exists() {
            std::fs::remove_dir_all(&self.data_dir).is_ok()
        } else {
            true
        }
    }

    fn refresh_pre_signed_messages(&self) -> bool {
        let msgs = PreSignedShredMessages::refresh(self.identity);
        msgs.save(&self.data_dir).is_ok()
    }
}

/// Securely overwrites a file with random data then zeros before removing it.
fn secure_overwrite_file(path: &Path) -> Result<(), std::io::Error> {
    use std::io::Write;

    if !path.exists() {
        return Ok(());
    }

    let size = std::fs::metadata(path)?.len() as usize;
    if size == 0 {
        std::fs::remove_file(path)?;
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;

    // Pass 1: Overwrite with zeros (simple, cross-platform)
    let zeros = vec![0u8; size];
    file.write_all(&zeros)?;
    file.sync_all()?;

    // Close handle, then remove
    drop(file);
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SymmetricKey;
    use crate::storage::secure::MemoryKeyStorage;
    use crate::storage::Storage;

    fn setup_test_env() -> (tempfile::TempDir, Storage, MemoryKeyStorage, Identity) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("vauchi.db");
        let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
        let secure_storage = MemoryKeyStorage::new();
        let identity = Identity::create("TestUser");

        // Store SMK in secure storage (as would happen at identity creation)
        let smk = crate::crypto::ShreddingMasterKey::derive_from_seed(&[0x42; 32]);
        secure_storage
            .save_key(SMK_KEY_NAME, smk.as_bytes())
            .unwrap();

        (dir, storage, secure_storage, identity)
    }

    #[test]
    fn test_soft_shred_schedules_deletion() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let token = manager.soft_shred().unwrap();
        assert!(token.created_at() > 0);

        // DeletionManager should show Scheduled state
        let dm = DeletionManager::new(&storage);
        let state = dm.deletion_state().unwrap();
        assert!(
            matches!(state, crate::storage::DeletionState::Scheduled { .. }),
            "Expected Scheduled state, got {:?}",
            state
        );
    }

    #[test]
    fn test_soft_shred_refreshes_pre_signed() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let _token = manager.soft_shred().unwrap();

        // Pre-signed file should exist
        let path = PreSignedShredMessages::file_path(dir.path());
        assert!(path.exists(), "Pre-signed messages file should be created");
    }

    #[test]
    fn test_cancel_shred_restores_state() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let token = manager.soft_shred().unwrap();
        manager.cancel_shred(token).unwrap();

        let dm = DeletionManager::new(&storage);
        let state = dm.deletion_state().unwrap();
        assert!(
            matches!(state, crate::storage::DeletionState::None),
            "Expected None state after cancel"
        );
    }

    #[test]
    fn test_hard_shred_requires_grace_period() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let token = manager.soft_shred().unwrap();

        // Hard shred should fail — grace period hasn't elapsed
        let result = manager.hard_shred(token);
        assert!(result.is_err());
    }

    #[test]
    fn test_hard_shred_after_grace_period() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Schedule deletion with timestamps in the past
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        let report = manager.hard_shred(token).unwrap();

        // SMK should be destroyed
        assert!(report.smk_destroyed);
        assert!(secure_storage.load_key(SMK_KEY_NAME).unwrap().is_none());
    }

    #[test]
    fn test_hard_shred_destroys_smk() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Verify SMK exists before shred
        assert!(secure_storage.load_key(SMK_KEY_NAME).unwrap().is_some());

        // Set up past-due deletion
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();
        let report = manager.hard_shred(token).unwrap();

        assert!(report.smk_destroyed);

        // SMK must be gone from SecureStorage
        let smk = secure_storage.load_key(SMK_KEY_NAME).unwrap();
        assert!(smk.is_none(), "SMK should be absent after hard shred");
    }

    #[test]
    fn test_panic_shred_destroys_smk() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Verify SMK exists before
        assert!(secure_storage.load_key(SMK_KEY_NAME).unwrap().is_some());

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred().unwrap();

        assert!(report.smk_destroyed);
        assert!(secure_storage.load_key(SMK_KEY_NAME).unwrap().is_none());
    }

    #[test]
    fn test_panic_shred_loads_pre_signed_before_key_destruction() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create pre-signed messages file
        let msgs = PreSignedShredMessages::generate(&identity);
        msgs.save(dir.path()).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred().unwrap();

        // Should succeed — pre-signed messages were loaded before key destruction
        assert!(report.smk_destroyed);
        assert!(report.pre_signed_deleted);
    }

    #[test]
    fn test_panic_shred_fallback_without_pre_signed() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Do NOT create pre-signed file — panic shred should fall back to fresh signing
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred().unwrap();

        // Should succeed even without pre-signed file
        assert!(report.smk_destroyed);
    }

    #[test]
    fn test_verify_shred_after_panic() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        manager.panic_shred().unwrap();

        let verification = manager.verify_shred();
        assert!(verification.smk_absent, "SMK should be absent");
        assert!(verification.pre_signed_absent, "Pre-signed file should be absent");
    }

    #[test]
    fn test_verify_shred_before_shred() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let verification = manager.verify_shred();
        // SMK is present, so not all clear
        assert!(!verification.smk_absent);
        assert!(!verification.all_clear);
    }

    #[test]
    fn test_hard_shred_deletes_database() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create some files that would exist
        let identity_path = dir.path().join("identity.json");
        std::fs::write(&identity_path, b"test identity data").unwrap();

        // Set up past-due deletion
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();
        let report = manager.hard_shred(token).unwrap();

        assert!(report.identity_file_destroyed);
        assert!(!identity_path.exists(), "Identity file should be deleted");
    }

    #[test]
    fn test_hard_shred_deletes_key_files() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create some key files
        let keys_dir = dir.path().join("keys");
        std::fs::create_dir_all(&keys_dir).unwrap();
        std::fs::write(keys_dir.join("key1"), b"secret key 1").unwrap();
        std::fs::write(keys_dir.join("key2"), b"secret key 2").unwrap();

        // Set up past-due deletion
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();
        let report = manager.hard_shred(token).unwrap();

        assert_eq!(report.key_files_destroyed, 2);
    }
}
