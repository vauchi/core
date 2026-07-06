// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync domain persistence view.
//!
//! Per decision (a) of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`, `SyncStore` owns every
//! `*_sync_*` table plus `version_vector` and the checkpoint tables, even when
//! the row keys reference another domain (e.g. `contact_sync_timestamps`).
//! Cross-domain cleanups other domains used to do inline — contact deletion's
//! sync-timestamp purge, device reset's sync-table wipe — are exposed here as
//! [`SyncStore::forget_contact`] and [`SyncStore::wipe_for_device_reset`] so
//! the coupling is an explicit cross-store call rather than a hidden write.

use std::collections::HashMap;
use std::sync::Arc;

use rusqlite::{Connection, params};

use super::super::{Storage, StorageError};
use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use crate::sync::device_sync::{FieldStamp, InterDeviceSyncState, SyncItem, VersionVector};

/// Scoped persistence view for the sync domain: inter-device sync state, the
/// local version vector, conflict-resolution field timestamps, resumable sync
/// and batch checkpoints, and per-contact last-sync timestamps.
pub struct SyncStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the sync domain.
    ///
    /// The returned [`SyncStore`] borrows this storage's connection,
    /// encryption key, and clock; it cannot outlive `self` and reaches only
    /// the sync-owned tables.
    pub fn sync(&self) -> SyncStore<'_> {
        SyncStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl SyncStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }

    /// Saves inter-device sync state for a specific device (encrypted).
    pub fn save_device_sync_state(&self, state: &InterDeviceSyncState) -> Result<(), StorageError> {
        let state_json = state.to_json();
        let state_encrypted = crate::crypto::encrypt(self.key, state_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO device_sync_state (device_id, state_json, state_json_encrypted, last_sync_version, updated_at)
             VALUES (?1, '', ?2, ?3, ?4)",
            params![
                state.device_id().as_slice(),
                state_encrypted,
                state.last_sync_version() as i64,
                now as i64,
            ],
        )?;
        Ok(())
    }

    /// Loads inter-device sync state for a specific device (decrypted).
    pub fn load_device_sync_state(
        &self,
        device_id: &[u8; 32],
    ) -> Result<Option<InterDeviceSyncState>, StorageError> {
        let result = self.conn.query_row(
            "SELECT state_json_encrypted, state_json FROM device_sync_state WHERE device_id = ?1",
            params![device_id.as_slice()],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let state = InterDeviceSyncState::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                let state = InterDeviceSyncState::from_json(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Lists all inter-device sync states (decrypted).
    pub fn list_device_sync_states(&self) -> Result<Vec<InterDeviceSyncState>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT state_json_encrypted, state_json FROM device_sync_state")?;

        // Site 2 peer (silent-failures audit): the original
        // `.filter_map(|r| r.ok())` silently dropped row read errors,
        // returning a short list under storage faults. Propagate so the
        // caller can distinguish "no sync states" from "storage failed".
        let rows: Vec<(Option<Vec<u8>>, String)> = stmt
            .query_map([], |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            })?
            .collect::<Result<_, _>>()?;

        let mut states = Vec::new();
        for (encrypted, plaintext) in rows {
            let json = if let Some(enc) = encrypted
                && !enc.is_empty()
            {
                let decrypted = crate::crypto::decrypt(self.key, &enc)
                    .map_err(|e| StorageError::Encryption(format!("Decrypt sync state: {}", e)))?;
                String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?
            } else {
                plaintext
            };
            if !json.is_empty()
                && let Ok(state) = InterDeviceSyncState::from_json(&json)
            {
                states.push(state);
            }
        }

        Ok(states)
    }

    /// Deletes inter-device sync state for a specific device.
    pub fn delete_device_sync_state(&self, device_id: &[u8; 32]) -> Result<bool, StorageError> {
        let rows = self.conn.execute(
            "DELETE FROM device_sync_state WHERE device_id = ?1",
            params![device_id.as_slice()],
        )?;
        Ok(rows > 0)
    }

    /// Saves the local version vector (encrypted).
    pub fn save_version_vector(&self, vector: &VersionVector) -> Result<(), StorageError> {
        let vector_json = vector.to_json();
        let encrypted = crate::crypto::encrypt(self.key, vector_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO version_vector (id, vector_json, vector_json_encrypted, updated_at)
             VALUES (1, '', ?1, ?2)",
            params![encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Loads the local version vector (decrypted).
    pub fn load_version_vector(&self) -> Result<Option<VersionVector>, StorageError> {
        let result = self.conn.query_row(
            "SELECT vector_json_encrypted, vector_json FROM version_vector WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let vector = VersionVector::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(vector))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                let vector = VersionVector::from_json(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(vector))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Saves the conflict-resolution field timestamps (encrypted).
    ///
    /// Persists the orchestrator's `field_timestamps` map (conflict-key ->
    /// last-write Unix-ms) so the LWW gate in
    /// `DeviceSyncOrchestrator::process_incoming` survives across sync
    /// cycles. Without this a reloaded orchestrator starts with empty
    /// timestamps and would let an older incoming change overwrite a newer
    /// local one (G3 of `2026-06-06-multi-device-sync-live-wiring`).
    pub fn save_field_timestamps(
        &self,
        timestamps: &HashMap<String, FieldStamp>,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(timestamps)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(self.key, json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_field_timestamps \
             (id, timestamps_json_encrypted, updated_at) VALUES (1, ?1, ?2)",
            params![encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Loads the conflict-resolution field timestamps (decrypted).
    ///
    /// Returns an empty map if none have been persisted yet.
    pub fn load_field_timestamps(&self) -> Result<HashMap<String, FieldStamp>, StorageError> {
        let result = self.conn.query_row(
            "SELECT timestamps_json_encrypted FROM sync_field_timestamps WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                Ok(encrypted)
            },
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                // Current shape: `{key: FieldStamp}`. Fall back to the
                // pre-tie-break G3 shape `{key: u64}` so a device upgraded
                // through G3 keeps its data (legacy device_id defaults to
                // all-zero, the lowest tie-break rank).
                if let Ok(map) = serde_json::from_str::<HashMap<String, FieldStamp>>(&json) {
                    Ok(map)
                } else {
                    let legacy: HashMap<String, u64> = serde_json::from_str(&json)
                        .map_err(|e| StorageError::Serialization(e.to_string()))?;
                    Ok(legacy
                        .into_iter()
                        .map(|(k, ts)| {
                            (
                                k,
                                FieldStamp {
                                    timestamp: ts,
                                    device_id: [0u8; 32],
                                },
                            )
                        })
                        .collect())
                }
            }
            Ok(_) => Ok(HashMap::new()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(HashMap::new()),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Saves a sync checkpoint for a target device (encrypted).
    ///
    /// Stores the list of sync items and how many have been sent so far,
    /// allowing sync to resume from the last checkpoint after interruption.
    pub fn save_sync_checkpoint(
        &self,
        target_device_id: &[u8; 32],
        items: &[SyncItem],
        sent_count: usize,
    ) -> Result<(), StorageError> {
        let items_json =
            serde_json::to_string(items).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(self.key, items_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO device_sync_checkpoints (target_device_id, items_json, items_json_encrypted, sent_count, updated_at)
             VALUES (?1, '', ?2, ?3, ?4)",
            params![
                target_device_id.as_slice(),
                encrypted,
                sent_count as i64,
                now as i64,
            ],
        )?;
        Ok(())
    }

    /// Loads a sync checkpoint for a target device (decrypted).
    ///
    /// Returns the list of sync items and the number already sent,
    /// or `None` if no checkpoint exists for this device.
    pub fn load_sync_checkpoint(
        &self,
        target_device_id: &[u8; 32],
    ) -> Result<Option<(Vec<SyncItem>, usize)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT items_json_encrypted, items_json, sent_count FROM device_sync_checkpoints WHERE target_device_id = ?1",
            params![target_device_id.as_slice()],
            |row| Ok((row.get::<_, Option<Vec<u8>>>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
        );

        match result {
            Ok((Some(encrypted), _, sent_count)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let items: Vec<SyncItem> = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some((items, sent_count as usize)))
            }
            Ok((_, plaintext, sent_count)) if !plaintext.is_empty() => {
                let items: Vec<SyncItem> = serde_json::from_str(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some((items, sent_count as usize)))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Clears a sync checkpoint for a target device.
    ///
    /// Called after sync completes successfully to remove the checkpoint.
    pub fn clear_sync_checkpoint(&self, target_device_id: &[u8; 32]) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM device_sync_checkpoints WHERE target_device_id = ?1",
            params![target_device_id.as_slice()],
        )?;
        Ok(())
    }

    /// Saves a batch checkpoint for crash recovery (encrypted).
    ///
    /// Tracks progress of a multi-item sync batch so it can be resumed
    /// after an interruption. Uses the batch_id as the logical grouping key.
    pub fn save_batch_checkpoint(
        &self,
        batch_id: &str,
        total_items: usize,
        processed_items: usize,
        state_json: &str,
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, state_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        let checkpoint_id = uuid::Uuid::new_v4().to_string(); // TODO(PFC): ambient UUID despite self.rng — see 2026-07-06-core-pfc-violations C5

        self.conn.execute(
            "INSERT OR REPLACE INTO sync_checkpoints
             (checkpoint_id, batch_id, total_items, processed_items, state_json, state_json_encrypted, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, '', ?5, ?6, ?6)",
            params![
                checkpoint_id,
                batch_id,
                total_items as i64,
                processed_items as i64,
                encrypted,
                now as i64,
            ],
        )?;
        Ok(())
    }

    /// Loads the latest batch checkpoint for a batch (decrypted).
    ///
    /// Returns (total_items, processed_items, state_json) or None if
    /// no checkpoint exists.
    pub fn load_batch_checkpoint(
        &self,
        batch_id: &str,
    ) -> Result<Option<(usize, usize, String)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT total_items, processed_items, state_json_encrypted, state_json FROM sync_checkpoints
             WHERE batch_id = ?1 ORDER BY updated_at DESC LIMIT 1",
            params![batch_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        );

        match result {
            Ok((total, processed, Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let state = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some((total as usize, processed as usize, state)))
            }
            Ok((total, processed, _, plaintext)) if !plaintext.is_empty() => {
                Ok(Some((total as usize, processed as usize, plaintext)))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Updates the progress of an existing batch checkpoint (encrypted).
    pub fn update_batch_checkpoint(
        &self,
        batch_id: &str,
        processed_items: usize,
        state_json: &str,
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, state_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "UPDATE sync_checkpoints SET processed_items = ?1, state_json = '', state_json_encrypted = ?2, updated_at = ?3
             WHERE batch_id = ?4",
            params![processed_items as i64, encrypted, now as i64, batch_id],
        )?;
        Ok(())
    }

    /// Clears all checkpoints for a batch after successful completion.
    pub fn clear_batch_checkpoint(&self, batch_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM sync_checkpoints WHERE batch_id = ?1",
            params![batch_id],
        )?;
        Ok(())
    }

    /// Records the last successful sync timestamp for a contact (encrypted).
    ///
    /// Uses a separate table from contacts to allow tracking sync timestamps
    /// independently of whether the contact exists in the contacts table.
    pub fn set_contact_last_sync(
        &self,
        contact_id: &str,
        timestamp: u64,
    ) -> Result<(), StorageError> {
        let ts_bytes = (timestamp as i64).to_le_bytes();
        let encrypted = crate::crypto::encrypt(self.key, &ts_bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO contact_sync_timestamps (contact_id, last_sync_at, last_sync_at_encrypted)
             VALUES (?1, ?2, ?3)",
            params![contact_id, timestamp as i64, encrypted],
        )?;
        Ok(())
    }

    /// Gets the last sync timestamp for a contact (decrypted).
    ///
    /// Returns None if the contact hasn't been synced yet.
    pub fn get_contact_last_sync(&self, contact_id: &str) -> Result<Option<u64>, StorageError> {
        let result = self.conn.query_row(
            "SELECT last_sync_at_encrypted, last_sync_at FROM contact_sync_timestamps WHERE contact_id = ?1",
            params![contact_id],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: i64 = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                if decrypted.len() == 8 {
                    let ts = i64::from_le_bytes(
                        decrypted
                            .try_into()
                            .map_err(|_| StorageError::Encryption("Invalid timestamp".into()))?,
                    );
                    Ok(Some(ts as u64))
                } else {
                    Err(StorageError::Encryption("Invalid timestamp length".into()))
                }
            }
            Ok((_, plaintext)) => Ok(Some(plaintext as u64)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Drops the contact's sync-timestamp row.
    ///
    /// Owns the `contact_sync_timestamps` cleanup that contact deletion needs:
    /// a stale row wrongly gates sync on contact_id reuse (read with
    /// `.unwrap_or(0)` in `sync/state.rs`). See problem
    /// `2026-06-01-contact-delete-orphans`. Callers run this inside the same
    /// transaction as the contact-row delete.
    pub fn forget_contact(&self, contact_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM contact_sync_timestamps WHERE contact_id = ?1",
            params![contact_id],
        )?;
        Ok(())
    }

    /// Clears the sync-owned tables wiped during device reset / unlink.
    ///
    /// Owns the sync-table portion of `Storage::wipe_device_data`
    /// (`device_sync_state`, `device_sync_checkpoints`,
    /// `sync_field_timestamps`); the device-info row is dropped by the
    /// device domain.
    pub fn wipe_for_device_reset(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM device_sync_state", [])?;
        self.conn
            .execute("DELETE FROM device_sync_checkpoints", [])?;
        self.conn.execute("DELETE FROM sync_field_timestamps", [])?;
        Ok(())
    }
}
