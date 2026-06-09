// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery storage operations.
//!
//! These methods forward to [`RecoveryStore`](super::RecoveryStore), the
//! domain-scoped persistence view. They are retained while call sites migrate
//! to `storage.recovery().*` (Phase 2 of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`) and will be removed once
//! that migration completes.

use super::{Storage, StorageError};
use crate::recovery::{RecoveryProgress, RecoverySettings};

impl Storage {
    /// Forwards to [`RecoveryStore::save_recovery_response`].
    pub fn save_recovery_response(
        &self,
        claim_id: &str,
        contact_id: &str,
        response: &str,
        remind_at: Option<u64>,
    ) -> Result<(), StorageError> {
        self.recovery()
            .save_recovery_response(claim_id, contact_id, response, remind_at)
    }

    /// Forwards to [`RecoveryStore::get_recovery_response`].
    pub fn get_recovery_response(
        &self,
        claim_id: &str,
    ) -> Result<Option<(String, String, Option<u64>)>, StorageError> {
        self.recovery().get_recovery_response(claim_id)
    }

    /// Forwards to [`RecoveryStore::check_recovery_rate_limit`].
    pub fn check_recovery_rate_limit(
        &self,
        identity_pk: &[u8],
    ) -> Result<(u32, u64), StorageError> {
        self.recovery().check_recovery_rate_limit(identity_pk)
    }

    /// Forwards to [`RecoveryStore::update_recovery_rate_limit`].
    pub fn update_recovery_rate_limit(
        &self,
        identity_pk: &[u8],
        count: u32,
        window_start: u64,
    ) -> Result<(), StorageError> {
        self.recovery()
            .update_recovery_rate_limit(identity_pk, count, window_start)
    }

    /// Forwards to [`RecoveryStore::save_recovery_settings`].
    pub fn save_recovery_settings(&self, settings: &RecoverySettings) -> Result<(), StorageError> {
        self.recovery().save_recovery_settings(settings)
    }

    /// Forwards to [`RecoveryStore::load_recovery_settings`].
    pub fn load_recovery_settings(&self) -> Result<Option<RecoverySettings>, StorageError> {
        self.recovery().load_recovery_settings()
    }

    /// Forwards to [`RecoveryStore::save_recovery_progress`].
    pub fn save_recovery_progress(&self, progress: &RecoveryProgress) -> Result<(), StorageError> {
        self.recovery().save_recovery_progress(progress)
    }

    /// Forwards to [`RecoveryStore::load_recovery_progress`].
    pub fn load_recovery_progress(&self) -> Result<Option<RecoveryProgress>, StorageError> {
        self.recovery().load_recovery_progress()
    }

    /// Forwards to [`RecoveryStore::clear_recovery_progress`].
    pub fn clear_recovery_progress(&self) -> Result<(), StorageError> {
        self.recovery().clear_recovery_progress()
    }
}
