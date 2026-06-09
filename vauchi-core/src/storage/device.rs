// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device and sync state storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::identity::device::DeviceRegistry;
use crate::sync::device_sync::{FieldStamp, InterDeviceSyncState, VersionVector};

impl Storage {
    /// Saves current device info (encrypted).
    pub fn save_device_info(
        &self,
        device_id: &[u8; 32],
        device_index: u32,
        device_name: &str,
        created_at: u64,
    ) -> Result<(), StorageError> {
        let json = serde_json::json!({
            "device_id": device_id.as_slice(),
            "device_index": device_index,
            "device_name": device_name,
            "created_at": created_at,
        });
        let json_bytes =
            serde_json::to_vec(&json).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(&self.encryption_key, &json_bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO device_info (id, device_id, device_index, device_name, created_at, device_info_encrypted)
             VALUES (1, ?1, ?2, '', ?3, ?4)",
            params![
                device_id.as_slice(),
                device_index as i32,
                created_at as i64,
                encrypted,
            ],
        )?;
        Ok(())
    }

    /// Loads current device info (decrypted).
    /// Returns (device_id, device_index, device_name, created_at) if found.
    #[allow(clippy::type_complexity)]
    pub fn load_device_info(&self) -> Result<Option<([u8; 32], u32, String, u64)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT device_info_encrypted, device_id, device_index, device_name, created_at FROM device_info WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<Vec<u8>>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        );

        match result {
            Ok((Some(encrypted), _, _, _, _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json: serde_json::Value = serde_json::from_slice(&decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let device_id_vec: Vec<u8> = serde_json::from_value(json["device_id"].clone())
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let device_id: [u8; 32] = device_id_vec
                    .try_into()
                    .map_err(|_| StorageError::Encryption("Invalid device ID length".into()))?;
                let device_index = json["device_index"].as_u64().unwrap_or(0) as u32;
                let device_name = json["device_name"].as_str().unwrap_or("").to_string();
                let created_at = json["created_at"].as_u64().unwrap_or(0);
                Ok(Some((device_id, device_index, device_name, created_at)))
            }
            Ok((_, device_id_vec, device_index, device_name, created_at))
                if !device_name.is_empty() =>
            {
                // Plaintext fallback for pre-v14 data
                let device_id: [u8; 32] = device_id_vec
                    .try_into()
                    .map_err(|_| StorageError::Encryption("Invalid device ID length".into()))?;
                Ok(Some((
                    device_id,
                    device_index as u32,
                    device_name,
                    created_at as u64,
                )))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Checks if device info exists.
    pub fn has_device_info(&self) -> Result<bool, StorageError> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM device_info WHERE id = 1", [], |row| {
                    row.get(0)
                })?;
        Ok(count > 0)
    }

    /// Saves the device registry (encrypted).
    pub fn save_device_registry(&self, registry: &DeviceRegistry) -> Result<(), StorageError> {
        let registry_json = serde_json::to_string(registry)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let registry_encrypted =
            crate::crypto::encrypt(&self.encryption_key, registry_json.as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO device_registry (id, registry_json, registry_json_encrypted, version, updated_at)
             VALUES (1, '', ?1, ?2, ?3)",
            params![registry_encrypted, registry.version() as i64, now as i64,],
        )?;
        Ok(())
    }

    /// Loads the device registry (decrypted).
    pub fn load_device_registry(&self) -> Result<Option<DeviceRegistry>, StorageError> {
        let result = self.conn.query_row(
            "SELECT registry_json_encrypted, registry_json FROM device_registry WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let registry: DeviceRegistry = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(registry))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                let registry: DeviceRegistry = serde_json::from_str(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(registry))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Loads the raw device registry JSON string.
    ///
    /// Used by GDPR export to include device information without
    /// requiring knowledge of the DeviceRegistry struct.
    pub fn load_device_registry_json(&self) -> Result<Option<String>, StorageError> {
        let result = self.conn.query_row(
            "SELECT registry_json_encrypted, registry_json FROM device_registry WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(json))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => Ok(Some(plaintext)),
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Checks if device registry exists.
    pub fn has_device_registry(&self) -> Result<bool, StorageError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM device_registry WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Saves inter-device sync state for a specific device (encrypted).
    pub fn save_device_sync_state(&self, state: &InterDeviceSyncState) -> Result<(), StorageError> {
        self.sync().save_device_sync_state(state)
    }

    /// Loads inter-device sync state for a specific device (decrypted).
    pub fn load_device_sync_state(
        &self,
        device_id: &[u8; 32],
    ) -> Result<Option<InterDeviceSyncState>, StorageError> {
        self.sync().load_device_sync_state(device_id)
    }

    /// Lists all inter-device sync states (decrypted).
    pub fn list_device_sync_states(&self) -> Result<Vec<InterDeviceSyncState>, StorageError> {
        self.sync().list_device_sync_states()
    }

    /// Deletes inter-device sync state for a specific device.
    pub fn delete_device_sync_state(&self, device_id: &[u8; 32]) -> Result<bool, StorageError> {
        self.sync().delete_device_sync_state(device_id)
    }

    /// Saves the local version vector (encrypted).
    pub fn save_version_vector(&self, vector: &VersionVector) -> Result<(), StorageError> {
        self.sync().save_version_vector(vector)
    }

    /// Loads the local version vector (decrypted).
    pub fn load_version_vector(&self) -> Result<Option<VersionVector>, StorageError> {
        self.sync().load_version_vector()
    }

    /// Saves the conflict-resolution field timestamps (encrypted).
    pub fn save_field_timestamps(
        &self,
        timestamps: &std::collections::HashMap<String, FieldStamp>,
    ) -> Result<(), StorageError> {
        self.sync().save_field_timestamps(timestamps)
    }

    /// Loads the conflict-resolution field timestamps (decrypted).
    pub fn load_field_timestamps(
        &self,
    ) -> Result<std::collections::HashMap<String, FieldStamp>, StorageError> {
        self.sync().load_field_timestamps()
    }

    /// Wipes all device-specific data from storage.
    ///
    /// Drops the `device_info` row (device-owned) and delegates the sync-owned
    /// tables (`device_sync_state`, `device_sync_checkpoints`,
    /// `sync_field_timestamps`) to [`SyncStore::wipe_for_device_reset`]. Used
    /// during identity deletion or device unlinking.
    pub fn wipe_device_data(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM device_info", [])?;
        self.sync().wipe_for_device_reset()
    }

    /// Saves a sync checkpoint for a target device (encrypted).
    ///
    /// Stores the list of sync items and how many have been sent so far,
    /// allowing sync to resume from the last checkpoint after interruption.
    pub fn save_sync_checkpoint(
        &self,
        target_device_id: &[u8; 32],
        items: &[crate::sync::device_sync::SyncItem],
        sent_count: usize,
    ) -> Result<(), StorageError> {
        self.sync()
            .save_sync_checkpoint(target_device_id, items, sent_count)
    }

    /// Loads a sync checkpoint for a target device (decrypted).
    ///
    /// Returns the list of sync items and the number already sent,
    /// or `None` if no checkpoint exists for this device.
    pub fn load_sync_checkpoint(
        &self,
        target_device_id: &[u8; 32],
    ) -> Result<Option<(Vec<crate::sync::device_sync::SyncItem>, usize)>, StorageError> {
        self.sync().load_sync_checkpoint(target_device_id)
    }

    /// Clears a sync checkpoint for a target device.
    ///
    /// Called after sync completes successfully to remove the checkpoint.
    pub fn clear_sync_checkpoint(&self, target_device_id: &[u8; 32]) -> Result<(), StorageError> {
        self.sync().clear_sync_checkpoint(target_device_id)
    }

    /// Saves a batch checkpoint for crash recovery (encrypted).
    pub fn save_batch_checkpoint(
        &self,
        batch_id: &str,
        total_items: usize,
        processed_items: usize,
        state_json: &str,
    ) -> Result<(), StorageError> {
        self.sync()
            .save_batch_checkpoint(batch_id, total_items, processed_items, state_json)
    }

    /// Loads the latest batch checkpoint for a batch (decrypted).
    pub fn load_batch_checkpoint(
        &self,
        batch_id: &str,
    ) -> Result<Option<(usize, usize, String)>, StorageError> {
        self.sync().load_batch_checkpoint(batch_id)
    }

    /// Updates the progress of an existing batch checkpoint (encrypted).
    pub fn update_batch_checkpoint(
        &self,
        batch_id: &str,
        processed_items: usize,
        state_json: &str,
    ) -> Result<(), StorageError> {
        self.sync()
            .update_batch_checkpoint(batch_id, processed_items, state_json)
    }

    /// Clears all checkpoints for a batch after successful completion.
    pub fn clear_batch_checkpoint(&self, batch_id: &str) -> Result<(), StorageError> {
        self.sync().clear_batch_checkpoint(batch_id)
    }

    // === Replay Nonce Operations (V3 replay_nonces) ===

    /// Saves a replay nonce for a contact.
    ///
    /// Uses INSERT OR IGNORE to be idempotent if the nonce already exists.
    pub fn save_replay_nonce(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO replay_nonces (contact_id, nonce, timestamp)
             VALUES (?1, ?2, ?3)",
            params![contact_id, nonce.as_slice(), timestamp as i64],
        )?;
        Ok(())
    }

    /// Test-only: insert a replay-nonce row with arbitrary (possibly
    /// malformed) BLOB length, used to exercise the
    /// [`Storage::load_replay_nonces`] error-propagation path (site 2 of
    /// `2026-05-21-silent-failures-in-security-paths`). Pre-2026-05-23
    /// the loader silently filtered such rows via `.filter_map`, opening
    /// an ADR-029 replay-defense gap under storage corruption.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert_malformed_replay_nonce(
        &self,
        contact_id: &str,
        bad_nonce: &[u8],
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO replay_nonces (contact_id, nonce, timestamp)
             VALUES (?1, ?2, ?3)",
            params![contact_id, bad_nonce, timestamp as i64],
        )?;
        Ok(())
    }

    /// Checks whether a replay nonce has already been recorded for a contact.
    ///
    /// Returns `true` if the nonce exists (i.e., this is a replay), `false` if fresh.
    pub fn is_replay_nonce(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
    ) -> Result<bool, StorageError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM replay_nonces WHERE contact_id = ?1 AND nonce = ?2",
            params![contact_id, nonce.as_slice()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Loads all replay nonces for a contact.
    ///
    /// Returns (nonce, timestamp) pairs.
    pub fn load_replay_nonces(
        &self,
        contact_id: &str,
    ) -> Result<Vec<([u8; 32], u64)>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT nonce, timestamp FROM replay_nonces WHERE contact_id = ?1 ORDER BY timestamp",
        )?;

        // Site 2 of `2026-05-21-silent-failures-in-security-paths`: row
        // read errors and `nonce_vec.try_into().ok()?` both used to be
        // silently dropped via `.filter_map`. A corrupted nonce row would
        // produce an empty/short set, opening an ADR-029 replay-defense
        // window without surfacing the storage fault anywhere. Propagate
        // both classes of error instead — a single corrupted row aborts
        // the load with a typed `Err`, and the caller decides how to
        // recover (e.g. refuse to process inbound updates until storage
        // is repaired). Healthy storage is unaffected.
        let raw_rows: Vec<(Vec<u8>, i64)> = stmt
            .query_map(params![contact_id], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<_, _>>()?;

        let mut nonces = Vec::with_capacity(raw_rows.len());
        for (nonce_vec, ts) in raw_rows {
            let actual_len = nonce_vec.len();
            let nonce: [u8; 32] = nonce_vec.try_into().map_err(|_| {
                StorageError::Serialization(format!(
                    "replay_nonces row has malformed nonce: expected 32 bytes, got {actual_len}"
                ))
            })?;
            nonces.push((nonce, ts as u64));
        }

        Ok(nonces)
    }

    /// Removes replay nonces older than the given cutoff timestamp.
    ///
    /// Returns the number of nonces removed.
    pub fn cleanup_replay_nonces(&self, cutoff: u64) -> Result<usize, StorageError> {
        let removed = self.conn.execute(
            "DELETE FROM replay_nonces WHERE timestamp < ?1",
            params![cutoff as i64],
        )?;
        Ok(removed)
    }
}
