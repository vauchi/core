// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`EmergencyStore`](super::EmergencyStore).

use super::{Storage, StorageError};
use crate::types::EmergencyBroadcastConfig;

impl Storage {
    /// Saves emergency broadcast configuration.
    ///
    /// Uses INSERT OR REPLACE for idempotent saves (singleton table, id=1).
    /// The trusted_contact_ids and message are encrypted before persisting.
    pub fn save_emergency_config(
        &self,
        config: &EmergencyBroadcastConfig,
    ) -> Result<(), StorageError> {
        self.emergency().save_emergency_config(config)
    }
    /// Loads emergency broadcast configuration.
    ///
    /// Returns `None` if no configuration has been set.
    pub fn load_emergency_config(&self) -> Result<Option<EmergencyBroadcastConfig>, StorageError> {
        self.emergency().load_emergency_config()
    }
    /// Deletes emergency broadcast configuration.
    pub fn delete_emergency_config(&self) -> Result<(), StorageError> {
        self.emergency().delete_emergency_config()
    }
}
