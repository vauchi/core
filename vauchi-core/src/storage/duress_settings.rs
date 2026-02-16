// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress Settings Storage
//!
//! CRUD operations for the `duress_settings` table (migration V20).
//! Stores encrypted configuration for duress alerts: which contacts
//! to alert, what message to send, and whether to include location.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::api::duress::DuressSettings;

impl Storage {
    /// Saves duress alert settings.
    ///
    /// Uses INSERT OR REPLACE for idempotent saves (singleton table, id=1).
    /// The alert_contact_ids and alert_message are encrypted before persisting.
    pub fn save_duress_settings(&self, settings: &DuressSettings) -> Result<(), StorageError> {
        let contact_ids_json = serde_json::to_vec(&settings.alert_contact_ids)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let contact_ids_encrypted = crate::crypto::encrypt(&self.encryption_key, &contact_ids_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let message_encrypted =
            crate::crypto::encrypt(&self.encryption_key, settings.alert_message.as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO duress_settings (id, alert_contact_ids_encrypted, alert_message_encrypted, include_location, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![
                contact_ids_encrypted,
                message_encrypted,
                settings.include_location as i32,
                now as i64,
                now as i64,
            ],
        )?;

        Ok(())
    }

    /// Loads duress alert settings.
    ///
    /// Returns `None` if no settings have been configured.
    pub fn load_duress_settings(&self) -> Result<Option<DuressSettings>, StorageError> {
        let result = self.conn.query_row(
            "SELECT alert_contact_ids_encrypted, alert_message_encrypted, include_location FROM duress_settings WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i32>(2)?,
                ))
            },
        );

        match result {
            Ok((contact_ids_encrypted, message_encrypted, include_location)) => {
                let contact_ids_json =
                    crate::crypto::decrypt(&self.encryption_key, &contact_ids_encrypted)
                        .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let alert_contact_ids: Vec<String> = serde_json::from_slice(&contact_ids_json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                let message_bytes =
                    crate::crypto::decrypt(&self.encryption_key, &message_encrypted)
                        .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let alert_message = String::from_utf8(message_bytes)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                Ok(Some(DuressSettings {
                    alert_contact_ids,
                    alert_message,
                    include_location: include_location != 0,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes duress alert settings.
    pub fn delete_duress_settings(&self) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM duress_settings WHERE id = 1", [])?;
        Ok(())
    }
}
