// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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

use crate::api::deletion::{DeletionError, DeletionManager};
use crate::api::pre_signed::{PreSignedPurgeRequest, legacy_pre_signed_file};
use crate::identity::Identity;
use crate::storage::secure::SecureStorage;
use crate::storage::{DeletionState, Storage};

use super::storage::{destroy_storage_keys, secure_overwrite_file};
use super::{
    BOOTSTRAP_KEY_NAME, PurgeSender, RevocationSender, SMK_KEY_NAME, ShredError, ShredReport,
    ShredToken, ShredVerification,
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
    pub fn soft_shred(&self) -> Result<ShredToken, ShredError> {
        let dm = DeletionManager::new(self.storage);
        dm.schedule_deletion()?;

        Ok(ShredToken::new(self.storage.clock().unix_seconds()))
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

        // 2. Send revocation deliveries to contacts (best-effort, while keys
        //    alive). Each is a pre-built (token, blob) the relay actually
        //    delivers — unlike the old per-IdentityRevoked path the HTTP
        //    transport silently dropped.
        if let Some(sender) = revocation_sender {
            for (token, blob) in &deletion_result.deliveries {
                if sender
                    .send_revocation_delivery(token, blob, self.storage.clock().unix_seconds())
                    .unwrap_or(false)
                {
                    report.contacts_notified += 1;
                }
            }
        }
        report.devices_notified = 0;

        // Send relay purge if sender provided (before point-of-no-return)
        if let Some(sender) = purge_sender {
            let now = self.storage.clock().unix_seconds();
            let purge = PreSignedPurgeRequest::sign_now(self.identity, now);
            report.relay_purge_sent = sender.send_purge(&purge, now).unwrap_or(false);
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

    /// Panic Shred — immediate destruction of this device's data, no grace
    /// period, and nothing sent.
    ///
    /// An emergency wipe concerns one of the identity's devices: the others
    /// keep working, so neither its contacts nor the relay are told
    /// (vauchi/private#910, ADR-033 amendment 2026-10-29). Telling the whole
    /// identity is identity deletion's job ([`Self::hard_shred`]).
    pub fn panic_shred(&self) -> Result<ShredReport, ShredError> {
        let mut report = ShredReport {
            smk_destroyed: self.destroy_smk(),
            ..ShredReport::default()
        };
        report.identity_file_destroyed = self.secure_delete_identity_file();
        report.key_files_destroyed = self.delete_key_files();
        report.sqlite_destroyed = self.secure_delete_database();
        report.pre_signed_deleted = self.delete_pre_signed_file();
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

        let bootstrap_key_absent = self
            .secure_storage
            .load_key(BOOTSTRAP_KEY_NAME)
            .map(|k| k.is_none())
            .unwrap_or(true);
        let database_absent = !self.data_dir.join("vauchi.db").exists();
        let data_dir_absent = !self.data_dir.exists();
        let pre_signed_absent = !legacy_pre_signed_file(&self.data_dir).exists();

        let all_clear = smk_absent
            && bootstrap_key_absent
            && database_absent
            && pre_signed_absent
            && data_dir_absent;

        ShredVerification {
            smk_absent,
            bootstrap_key_absent,
            database_absent,
            data_dir_absent,
            pre_signed_absent,
            all_clear,
        }
    }

    // ── Internal helpers ──

    fn destroy_smk(&self) -> bool {
        destroy_storage_keys(self.secure_storage)
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
        // best-effort: keys overwritten above; directory removal failure
        // (FS busy, EACCES) leaves an empty dir but no recoverable data
        #[allow(clippy::let_underscore_must_use)]
        let _ = std::fs::remove_dir(&keys_dir);
        count
    }

    fn secure_delete_database(&self) -> bool {
        // Flush WAL into main DB before file-level overwrite (#81).
        // best-effort: checkpoint failure means WAL bytes survive in
        // -wal/-shm files, which are overwritten by the loop below.
        #[allow(clippy::let_underscore_must_use)]
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

        // Clean pre-migration backup files (F8 audit fix)
        if let Ok(entries) = std::fs::read_dir(&self.data_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.contains(".pre-migration-v")
                    && name_str.ends_with(".bak")
                    && secure_overwrite_file(&entry.path()).is_err()
                {
                    success = false;
                }
            }
        }

        success
    }

    fn delete_pre_signed_file(&self) -> bool {
        let path = legacy_pre_signed_file(&self.data_dir);
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
}
