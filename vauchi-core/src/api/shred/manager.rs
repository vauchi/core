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

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::deletion::{DeletionError, DeletionManager};
use crate::api::pre_signed::PreSignedShredMessages;
use crate::identity::Identity;
use crate::storage::secure::SecureStorage;
use crate::storage::{DeletionState, Storage};

use super::storage::secure_overwrite_file;
use super::{
    PurgeSender, RevocationSender, SMK_KEY_NAME, ShredError, ShredReport, ShredToken,
    ShredVerification,
};

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
        if let DeletionState::Scheduled { scheduled_at, .. } = &state
            && token.created_at() < *scheduled_at
        {
            return Err(ShredError::Deletion(DeletionError::DeletionFailed(
                "ShredToken predates scheduled deletion".to_string(),
            )));
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
                .map(|c| crate::network::IdentityRevoked::create(self.identity, c.id(), now))
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
