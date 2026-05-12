// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency Config Storage
//!
//! CRUD operations for the `emergency_config` table (migration V22).
//! Stores encrypted configuration for emergency broadcasts: which contacts
//! to alert, what message to send, and whether to include location.

use rusqlite::params;

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
        let contact_ids_json = serde_json::to_vec(&config.trusted_contact_ids)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let contact_ids_encrypted = crate::crypto::encrypt(&self.encryption_key, &contact_ids_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let message_encrypted =
            crate::crypto::encrypt(&self.encryption_key, config.message.as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO emergency_config (id, trusted_contact_ids_encrypted, message_encrypted, include_location, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![
                contact_ids_encrypted,
                message_encrypted,
                config.include_location as i32,
                now as i64,
                now as i64,
            ],
        )?;

        Ok(())
    }

    /// Loads emergency broadcast configuration.
    ///
    /// Returns `None` if no configuration has been set.
    pub fn load_emergency_config(&self) -> Result<Option<EmergencyBroadcastConfig>, StorageError> {
        let result = self.conn.query_row(
            "SELECT trusted_contact_ids_encrypted, message_encrypted, include_location FROM emergency_config WHERE id = 1",
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
                let trusted_contact_ids: Vec<String> = serde_json::from_slice(&contact_ids_json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                let message_bytes =
                    crate::crypto::decrypt(&self.encryption_key, &message_encrypted)
                        .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let message = String::from_utf8(message_bytes)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                Ok(Some(EmergencyBroadcastConfig {
                    trusted_contact_ids,
                    message,
                    include_location: include_location != 0,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes emergency broadcast configuration.
    pub fn delete_emergency_config(&self) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM emergency_config WHERE id = 1", [])?;
        Ok(())
    }
}
