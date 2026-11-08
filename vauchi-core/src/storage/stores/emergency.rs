// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Emergency domain persistence view (emergency_config).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use rusqlite::{Connection, params};
use std::sync::Arc;

use super::super::{Storage, StorageError};
use crate::emergency::EmergencyBroadcastConfig;

/// Scoped persistence view for the emergency domain.
pub struct EmergencyStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
    table: &'static str,
    /// Sealed after a write, for the mask profile's config: a write whose
    /// seal fails returns the error (vauchi/private#1003).
    sealer: Option<&'a Storage>,
}

impl Storage {
    /// Scoped persistence view for the emergency domain.
    pub fn emergency(&self) -> crate::storage::Sealed<'_, EmergencyStore<'_>> {
        self.sealed({
            EmergencyStore {
                conn: &self.conn,
                key: &self.encryption_key,
                clock: &self.clock,
                table: "emergency_config",
                sealer: None,
            }
        })
    }

    /// The mask profile's emergency config, in the sealed mask database
    /// (ADR-080). It starts not set up, as on a fresh install: nothing of the
    /// real one is copied.
    pub fn duress_emergency(&self) -> crate::storage::Sealed<'_, EmergencyStore<'_>> {
        let mask = self.mask_profile();
        self.mask_sealed(mask.sealed(EmergencyStore {
            conn: &mask.conn,
            key: &mask.encryption_key,
            clock: &mask.clock,
            table: "emergency_config",
            sealer: Some(self),
        }))
    }
}

impl EmergencyStore<'_> {
    fn seal_if_twin(&self) -> Result<(), StorageError> {
        self.sealer.map_or(Ok(()), Storage::seal_mask)
    }

    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }

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
        let contact_ids_encrypted = crate::crypto::encrypt(self.key, &contact_ids_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let message_encrypted = crate::crypto::encrypt(self.key, config.message.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            &format!("INSERT OR REPLACE INTO {} (id, trusted_contact_ids_encrypted, message_encrypted, include_location, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5)", self.table),
            params![
                contact_ids_encrypted,
                message_encrypted,
                config.include_location as i32,
                now as i64,
                now as i64,
            ],
        )?;
        self.seal_if_twin()
    }
    /// Loads emergency broadcast configuration.
    ///
    /// Returns `None` if no configuration has been set.
    pub fn load_emergency_config(&self) -> Result<Option<EmergencyBroadcastConfig>, StorageError> {
        let result = self.conn.query_row(
            &format!("SELECT trusted_contact_ids_encrypted, message_encrypted, include_location FROM {} WHERE id = 1", self.table),
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
                let contact_ids_json = crate::crypto::decrypt(self.key, &contact_ids_encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let trusted_contact_ids: Vec<String> = serde_json::from_slice(&contact_ids_json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                let message_bytes = crate::crypto::decrypt(self.key, &message_encrypted)
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
            .execute(&format!("DELETE FROM {} WHERE id = 1", self.table), [])?;
        self.seal_if_twin()
    }
}
