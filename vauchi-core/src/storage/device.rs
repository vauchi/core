// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device and sync state storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::identity::device::DeviceRegistry;
use crate::sync::device_sync::{InterDeviceSyncState, VersionVector};

impl Storage {
    // === Device Operations ===

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

    // === Device Sync State Operations ===

    /// Saves inter-device sync state for a specific device (encrypted).
    pub fn save_device_sync_state(&self, state: &InterDeviceSyncState) -> Result<(), StorageError> {
        let state_json = state.to_json();
        let state_encrypted = crate::crypto::encrypt(&self.encryption_key, state_json.as_bytes())
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
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
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
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &enc)
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

    // === Version Vector Operations ===

    /// Saves the local version vector (encrypted).
    pub fn save_version_vector(&self, vector: &VersionVector) -> Result<(), StorageError> {
        let vector_json = vector.to_json();
        let encrypted = crate::crypto::encrypt(&self.encryption_key, vector_json.as_bytes())
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
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
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

    // === Conflict-resolution field timestamps (G3) ===

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
        timestamps: &std::collections::HashMap<String, u64>,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(timestamps)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(&self.encryption_key, json.as_bytes())
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
    pub fn load_field_timestamps(
        &self,
    ) -> Result<std::collections::HashMap<String, u64>, StorageError> {
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
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let map = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(map)
            }
            Ok(_) => Ok(std::collections::HashMap::new()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(std::collections::HashMap::new()),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Device Data Wipe ===

    /// Wipes all device-specific data from storage.
    ///
    /// Deletes rows from: `device_info`, `device_sync_state`, and
    /// `device_sync_checkpoints`. This is used during identity deletion
    /// or device unlinking to ensure no device-specific data remains.
    pub fn wipe_device_data(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM device_info", [])?;
        self.conn.execute("DELETE FROM device_sync_state", [])?;
        self.conn
            .execute("DELETE FROM device_sync_checkpoints", [])?;
        self.conn.execute("DELETE FROM sync_field_timestamps", [])?;
        Ok(())
    }

    // === Sync Checkpoint Operations ===

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
        let items_json =
            serde_json::to_string(items).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(&self.encryption_key, items_json.as_bytes())
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
    ) -> Result<Option<(Vec<crate::sync::device_sync::SyncItem>, usize)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT items_json_encrypted, items_json, sent_count FROM device_sync_checkpoints WHERE target_device_id = ?1",
            params![target_device_id.as_slice()],
            |row| Ok((row.get::<_, Option<Vec<u8>>>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
        );

        match result {
            Ok((Some(encrypted), _, sent_count)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let items: Vec<crate::sync::device_sync::SyncItem> = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some((items, sent_count as usize)))
            }
            Ok((_, plaintext, sent_count)) if !plaintext.is_empty() => {
                let items: Vec<crate::sync::device_sync::SyncItem> =
                    serde_json::from_str(&plaintext)
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

    // === Batch Checkpoint Operations (V12 sync_checkpoints) ===

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
        let encrypted = crate::crypto::encrypt(&self.encryption_key, state_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        let checkpoint_id = uuid::Uuid::new_v4().to_string();

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
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
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
        let encrypted = crate::crypto::encrypt(&self.encryption_key, state_json.as_bytes())
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
