// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity storage forwarders to [`IdentityStore`](super::IdentityStore).

use super::{Storage, StorageError};

impl Storage {
    /// Saves identity backup data (encrypted).
    pub fn save_identity(
        &self,
        backup_data: &[u8],
        display_name: &str,
    ) -> Result<(), StorageError> {
        self.identity().save_identity(backup_data, display_name)
    }
    /// Loads identity backup data (decrypted).
    /// Returns (backup_data, display_name) if found.
    pub fn load_identity(&self) -> Result<Option<(Vec<u8>, String)>, StorageError> {
        self.identity().load_identity()
    }
    /// Checks if identity exists.
    pub fn has_identity(&self) -> Result<bool, StorageError> {
        self.identity().has_identity()
    }
    /// Deletes the persisted identity row.
    ///
    /// Used by `Vauchi::perform_emergency_wipe` to clear identity from
    /// storage. After this call, `has_identity()` returns `false` and
    /// `load_identity()` returns `None`. Idempotent — succeeds even if
    /// no row exists.
    pub fn delete_identity(&self) -> Result<(), StorageError> {
        self.identity().delete_identity()
    }
    /// Saves the app password hash and salt to the identity table.
    ///
    /// The hash is encrypted with the storage key before persisting.
    pub fn save_app_password(&self, hash: &[u8; 32], salt: &[u8; 16]) -> Result<(), StorageError> {
        self.identity().save_app_password(hash, salt)
    }
    /// Saves the duress password hash and salt, and enables duress mode.
    ///
    /// The hash is encrypted with the storage key before persisting.
    pub fn save_duress_password(
        &self,
        hash: &[u8; 32],
        salt: &[u8; 16],
    ) -> Result<(), StorageError> {
        self.identity().save_duress_password(hash, salt)
    }
    /// Loads the password configuration from the identity table.
    ///
    /// Returns `None` if no password has been set (all password columns are NULL).
    pub fn load_password_config(
        &self,
    ) -> Result<Option<crate::api::app_password::AppPasswordConfig>, StorageError> {
        self.identity().load_password_config()
    }
    /// Disables duress mode and clears duress hash/salt.
    pub fn disable_duress(&self) -> Result<(), StorageError> {
        self.identity().disable_duress()
    }
}
