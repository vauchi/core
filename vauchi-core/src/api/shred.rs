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

use crate::api::account::{DeletionError, DeletionManager};
use crate::api::pre_signed::{PreSignedPurgeRequest, PreSignedShredMessages};
use crate::identity::Identity;
use crate::storage::secure::SecureStorage;
use crate::storage::{DeletionState, Storage};

/// Trait for sending relay purge requests during shred operations.
///
/// Abstracted to allow testing without a real relay connection and to
/// decouple the shred orchestration from the network layer.
pub trait PurgeSender {
    /// Sends a pre-signed purge request to the relay.
    ///
    /// Returns `Ok(true)` if the relay acknowledged the purge,
    /// `Ok(false)` if the request was sent but not confirmed,
    /// or `Err` if sending failed entirely.
    fn send_purge(&mut self, purge: &PreSignedPurgeRequest) -> Result<bool, ShredError>;
}

/// Trait for sending identity revocation messages to contacts during shred.
///
/// Abstracted to allow testing without a real relay connection and to
/// decouple the shred orchestration from the network layer.
pub trait RevocationSender {
    /// Sends an identity revocation message to a contact via the relay.
    ///
    /// Returns `Ok(true)` if the relay acknowledged the message,
    /// `Ok(false)` if the message was sent but not confirmed,
    /// or `Err` if sending failed entirely.
    fn send_revocation(
        &mut self,
        revocation: &crate::network::AccountRevoked,
    ) -> Result<bool, ShredError>;
}

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
    #[error("Deletion error: {0}")]
    Deletion(#[from] DeletionError),

    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),

    #[error("Pre-signed messages unavailable: {0}")]
    PreSignedUnavailable(String),

    #[error("SMK destruction failed: {0}")]
    SmkDestructionFailed(String),

    #[error("File operation failed: {0}")]
    FileError(String),
}

/// Orchestrates cryptographic shredding of all identity data.
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
    ///
    /// If `purge_sender` is provided, sends a pre-signed purge request to the
    /// relay before destroying local data. Purge failure does not abort shred.
    pub fn hard_shred(
        &self,
        token: ShredToken,
        purge_sender: Option<&mut dyn PurgeSender>,
        revocation_sender: Option<&mut dyn RevocationSender>,
    ) -> Result<ShredReport, ShredError> {
        let mut report = ShredReport::default();

        // Validate ShredToken was created at or after the scheduled deletion time (#199a).
        // This ensures the token came from soft_shred(), not fabricated independently.
        let dm = DeletionManager::new(self.storage);
        let state = dm.deletion_state()?;
        if let DeletionState::Scheduled { scheduled_at, .. } = &state {
            if token.created_at() < *scheduled_at {
                return Err(ShredError::Deletion(DeletionError::DeletionFailed(
                    "ShredToken predates scheduled deletion".to_string(),
                )));
            }
        }

        // 1. Verify grace period has elapsed and generate revocations
        let deletion_result = dm.execute_deletion(self.identity)?;

        // 2. Send revocation notifications to contacts (best-effort, while keys alive)
        if let Some(sender) = revocation_sender {
            for revocation in &deletion_result.revocations {
                if sender.send_revocation(revocation).unwrap_or(false) {
                    report.contacts_notified += 1;
                }
            }
        }
        report.devices_notified = 0;

        // Send relay purge if sender provided (before point-of-no-return)
        if let Some(sender) = purge_sender {
            let pre_signed = PreSignedShredMessages::load(&self.data_dir)
                .or_else(|_| Ok::<_, ShredError>(PreSignedShredMessages::generate(self.identity)));
            if let Ok(msgs) = pre_signed {
                report.relay_purge_sent = sender.send_purge(&msgs.purge_request).unwrap_or(false);
            }
        }

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
    ///
    /// If `purge_sender` is provided, sends the pre-signed purge request to
    /// the relay after key destruction (Phase C). Purge failure does not abort shred.
    pub fn panic_shred(
        &self,
        purge_sender: Option<&mut dyn PurgeSender>,
        revocation_sender: Option<&mut dyn RevocationSender>,
    ) -> Result<ShredReport, ShredError> {
        let mut report = ShredReport::default();

        // ── Phase A: Prepare outbound messages while keys available ──

        // 1. Load pre-signed messages from file (unencrypted per DP-3)
        let pre_signed = match PreSignedShredMessages::load(&self.data_dir) {
            Ok(msgs) => Some(msgs),
            Err(_) => {
                // 2. Fallback: sign fresh messages now (keys still available)
                let msgs = PreSignedShredMessages::generate(self.identity);
                Some(msgs)
            }
        };

        // Sign fresh revocations for each contact (keys still available)
        let revocations = {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let contacts = self.storage.list_contacts().unwrap_or_default();
            contacts
                .iter()
                .map(|c| crate::network::AccountRevoked::create(self.identity, c.id(), now))
                .collect::<Vec<_>>()
        };

        // ── Phase B: Destroy all key material ──

        // 3. Destroy SMK from all SecureStorage backends
        report.smk_destroyed = self.destroy_smk();

        // 4. Secure-delete identity backup file
        report.identity_file_destroyed = self.secure_delete_identity_file();

        // 5. Delete all key files
        report.key_files_destroyed = self.delete_key_files();

        // ── Phase C: Best-effort network and cleanup ──
        // Send pre-signed purge after key destruction (best-effort)
        if let (Some(sender), Some(msgs)) = (purge_sender, &pre_signed) {
            report.relay_purge_sent = sender.send_purge(&msgs.purge_request).unwrap_or(false);
        }

        // Send revocations to contacts (best-effort, keys already destroyed)
        if let Some(sender) = revocation_sender {
            for revocation in &revocations {
                if sender.send_revocation(revocation).unwrap_or(false) {
                    report.contacts_notified += 1;
                }
            }
        }

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
        let pre_signed_absent = !PreSignedShredMessages::file_path(&self.data_dir).exists();

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
        self.secure_storage.secure_delete_key(SMK_KEY_NAME).is_ok()
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
        // Flush WAL into main DB before file-level overwrite (#81)
        let _ = self.storage.wal_checkpoint();

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
            // Use secure overwrite instead of bare remove_file (#200a)
            secure_overwrite_file(&path).is_ok()
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

/// Widget confirmation mode for panic shred activation.
///
/// Defines how the user confirms a panic shred from the home screen widget,
/// providing a safety mechanism against accidental triggers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WidgetConfirmationMode {
    /// Default: tap once, then confirm in a dialog.
    TapConfirm,
    /// Long press to trigger (no separate confirmation).
    LongPress,
    /// Double tap to trigger (no separate confirmation).
    DoubleTap,
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

/// Public entry point for secure file overwrite, callable from other modules.
pub(crate) fn secure_overwrite_file_public(path: &Path) -> Result<(), std::io::Error> {
    secure_overwrite_file(path)
}

/// Securely overwrites a file with random data then zeros before removing it.
///
/// Uses 2-pass overwrite (#200a): random data (destroys original bit patterns)
/// followed by zeros (verifiable wipe). Both passes are flushed to disk with
/// `sync_all()` to ensure the overwrite reaches physical storage.
fn secure_overwrite_file(path: &Path) -> Result<(), std::io::Error> {
    use aws_lc_rs::rand::SecureRandom;
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
    let rng = aws_lc_rs::rand::SystemRandom::new();
    let mut random = vec![0u8; size];
    rng.fill(&mut random)
        .map_err(|_| std::io::Error::other("RNG fill failed"))?;
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

// INLINE_TEST_REQUIRED: tests access private internals
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
        let result = manager.hard_shred(token, None, None);
        result.expect_err("expected error");
    }

    #[test]
    fn test_hard_shred_after_grace_period() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Schedule deletion with timestamps in the past
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        let report = manager.hard_shred(token, None, None).unwrap();

        // SMK should be destroyed
        assert!(report.smk_destroyed);
        assert!(secure_storage.load_key(SMK_KEY_NAME).unwrap().is_none());
    }

    #[test]
    fn test_hard_shred_destroys_smk() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Verify SMK exists before shred
        secure_storage
            .load_key(SMK_KEY_NAME)
            .unwrap()
            .expect("expected Some");

        // Set up past-due deletion
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();
        let report = manager.hard_shred(token, None, None).unwrap();

        assert!(report.smk_destroyed);

        // SMK must be gone from SecureStorage
        let smk = secure_storage.load_key(SMK_KEY_NAME).unwrap();
        assert!(smk.is_none(), "SMK should be absent after hard shred");
    }

    #[test]
    fn test_panic_shred_destroys_smk() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Verify SMK exists before
        secure_storage
            .load_key(SMK_KEY_NAME)
            .unwrap()
            .expect("expected Some");

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred(None, None).unwrap();

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
        let report = manager.panic_shred(None, None).unwrap();

        // Should succeed — pre-signed messages were loaded before key destruction
        assert!(report.smk_destroyed);
        assert!(report.pre_signed_deleted);
    }

    #[test]
    fn test_panic_shred_fallback_without_pre_signed() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Do NOT create pre-signed file — panic shred should fall back to fresh signing
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred(None, None).unwrap();

        // Should succeed even without pre-signed file
        assert!(report.smk_destroyed);
    }

    #[test]
    fn test_verify_shred_after_panic() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        manager.panic_shred(None, None).unwrap();

        let verification = manager.verify_shred();
        assert!(verification.smk_absent, "SMK should be absent");
        assert!(
            verification.pre_signed_absent,
            "Pre-signed file should be absent"
        );
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
        let report = manager.hard_shred(token, None, None).unwrap();

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
        let report = manager.hard_shred(token, None, None).unwrap();

        assert_eq!(report.key_files_destroyed, 2);
    }

    // === PurgeSender tests ===

    struct MockPurgeSender {
        purge_sent: bool,
        should_fail: bool,
    }

    impl MockPurgeSender {
        fn new() -> Self {
            Self {
                purge_sent: false,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                purge_sent: false,
                should_fail: true,
            }
        }
    }

    impl PurgeSender for MockPurgeSender {
        fn send_purge(&mut self, _purge: &PreSignedPurgeRequest) -> Result<bool, ShredError> {
            if self.should_fail {
                return Err(ShredError::FileError("mock purge failure".into()));
            }
            self.purge_sent = true;
            Ok(true)
        }
    }

    #[test]
    fn test_hard_shred_sends_purge_when_sender_provided() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        let mut sender = MockPurgeSender::new();
        let report = manager.hard_shred(token, Some(&mut sender), None).unwrap();

        assert!(sender.purge_sent, "Purge should have been sent");
        assert!(report.relay_purge_sent, "Report should reflect purge sent");
    }

    #[test]
    fn test_hard_shred_succeeds_when_purge_fails() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        let mut sender = MockPurgeSender::failing();
        let report = manager.hard_shred(token, Some(&mut sender), None).unwrap();

        assert!(
            !report.relay_purge_sent,
            "Purge failure should not abort shred"
        );
        assert!(report.smk_destroyed, "SMK should still be destroyed");
    }

    #[test]
    fn test_panic_shred_sends_purge() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create pre-signed messages file
        let msgs = PreSignedShredMessages::generate(&identity);
        msgs.save(dir.path()).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let mut sender = MockPurgeSender::new();
        let report = manager.panic_shred(Some(&mut sender), None).unwrap();

        assert!(sender.purge_sent, "Purge should have been sent");
        assert!(report.relay_purge_sent, "Report should reflect purge sent");
        assert!(report.smk_destroyed, "SMK should be destroyed");
    }

    #[test]
    fn test_shred_without_sender_still_works() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        // Pass None — backward compat
        let report = manager.hard_shred(token, None, None).unwrap();

        assert!(!report.relay_purge_sent);
        assert!(report.smk_destroyed);
    }

    // === RevocationSender tests ===

    struct MockRevocationSender {
        revocations_sent: std::cell::RefCell<Vec<String>>,
        should_fail: bool,
    }

    impl MockRevocationSender {
        fn new() -> Self {
            Self {
                revocations_sent: std::cell::RefCell::new(Vec::new()),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                revocations_sent: std::cell::RefCell::new(Vec::new()),
                should_fail: true,
            }
        }

        fn sent_count(&self) -> usize {
            self.revocations_sent.borrow().len()
        }
    }

    impl RevocationSender for MockRevocationSender {
        fn send_revocation(
            &mut self,
            revocation: &crate::network::AccountRevoked,
        ) -> Result<bool, ShredError> {
            if self.should_fail {
                return Err(ShredError::FileError("mock revocation failure".into()));
            }
            self.revocations_sent
                .borrow_mut()
                .push(revocation.recipient_id.clone());
            Ok(true)
        }
    }

    fn add_test_contact(storage: &Storage, _contact_id: &str) {
        use crate::contact::Contact;
        use crate::contact_card::ContactCard;
        use crate::crypto::SymmetricKey;

        let card = ContactCard::new("Test Contact");
        // Use a deterministic public key derived from the contact_id
        let mut public_key = [0u8; 32];
        let id_bytes = _contact_id.as_bytes();
        let len = id_bytes.len().min(32);
        public_key[..len].copy_from_slice(&id_bytes[..len]);

        let shared_key = SymmetricKey::generate();
        let contact = Contact::from_exchange(public_key, card, shared_key);
        storage.save_contact(&contact).unwrap();
    }

    #[test]
    fn test_hard_shred_sends_revocations() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Add test contacts
        add_test_contact(&storage, "contact_aaa");
        add_test_contact(&storage, "contact_bbb");

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        let mut sender = MockRevocationSender::new();
        let report = manager.hard_shred(token, None, Some(&mut sender)).unwrap();

        assert_eq!(report.contacts_notified, 2);
        assert_eq!(sender.sent_count(), 2);
    }

    #[test]
    fn test_hard_shred_revocation_failure_does_not_abort() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        let mut sender = MockRevocationSender::failing();
        let report = manager.hard_shred(token, None, Some(&mut sender)).unwrap();

        // Revocation failed but shred still succeeded
        assert_eq!(report.contacts_notified, 0);
        assert!(report.smk_destroyed, "SMK should still be destroyed");
    }

    #[test]
    fn test_panic_shred_sends_revocations() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");
        add_test_contact(&storage, "contact_bbb");
        add_test_contact(&storage, "contact_ccc");

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let mut sender = MockRevocationSender::new();
        let report = manager.panic_shred(None, Some(&mut sender)).unwrap();

        assert_eq!(report.contacts_notified, 3);
        assert_eq!(sender.sent_count(), 3);
        assert!(report.smk_destroyed);
    }

    #[test]
    fn test_panic_shred_revocation_failure_does_not_abort() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let mut sender = MockRevocationSender::failing();
        let report = manager.panic_shred(None, Some(&mut sender)).unwrap();

        assert_eq!(report.contacts_notified, 0);
        assert!(report.smk_destroyed, "SMK should still be destroyed");
    }

    #[test]
    fn test_hard_shred_no_revocation_sender_backward_compat() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new();

        // Pass None for revocation_sender — backward compat
        let report = manager.hard_shred(token, None, None).unwrap();

        assert_eq!(report.contacts_notified, 0);
        assert!(report.smk_destroyed);
    }
}
