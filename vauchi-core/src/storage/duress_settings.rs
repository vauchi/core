// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`DuressStore`](super::DuressStore).

use super::{Storage, StorageError};
use crate::types::DuressSettings;

impl Storage {
    /// Saves duress alert settings.
    ///
    /// Uses INSERT OR REPLACE for idempotent saves (singleton table, id=1).
    /// The alert_contact_ids and alert_message are encrypted before persisting.
    pub fn save_duress_settings(&self, settings: &DuressSettings) -> Result<(), StorageError> {
        self.duress().save_duress_settings(settings)
    }
    /// Loads duress alert settings.
    ///
    /// Returns `None` if no settings have been configured.
    pub fn load_duress_settings(&self) -> Result<Option<DuressSettings>, StorageError> {
        self.duress().load_duress_settings()
    }
    /// Deletes duress alert settings.
    pub fn delete_duress_settings(&self) -> Result<(), StorageError> {
        self.duress().delete_duress_settings()
    }
}
