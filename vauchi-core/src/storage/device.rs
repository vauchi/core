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

    /// Saves current device info.
    pub fn save_device_info(
        &self,
        device_id: &[u8; 32],
        device_index: u32,
        device_name: &str,
        created_at: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO device_info (id, device_id, device_index, device_name, created_at)
             VALUES (1, ?1, ?2, ?3, ?4)",
            params![
                device_id.as_slice(),
                device_index as i32,
                device_name,
                created_at as i64,
            ],
        )?;
        Ok(())
    }

    /// Loads current device info.
    /// Returns (device_id, device_index, device_name, created_at) if found.
    #[allow(clippy::type_complexity)]
    pub fn load_device_info(&self) -> Result<Option<([u8; 32], u32, String, u64)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT device_id, device_index, device_name, created_at FROM device_info WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        );

        match result {
            Ok((device_id_vec, device_index, device_name, created_at)) => {
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

    /// Saves the device registry.
    pub fn save_device_registry(&self, registry: &DeviceRegistry) -> Result<(), StorageError> {
        let registry_json = serde_json::to_string(registry)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO device_registry (id, registry_json, version, updated_at)
             VALUES (1, ?1, ?2, ?3)",
            params![registry_json, registry.version() as i64, now as i64,],
        )?;
        Ok(())
    }

    /// Loads the device registry.
    pub fn load_device_registry(&self) -> Result<Option<DeviceRegistry>, StorageError> {
        let result = self.conn.query_row(
            "SELECT registry_json FROM device_registry WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(json) => {
                let registry: DeviceRegistry = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(registry))
            }
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

    /// Saves inter-device sync state for a specific device.
    pub fn save_device_sync_state(&self, state: &InterDeviceSyncState) -> Result<(), StorageError> {
        let state_json = state.to_json();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO device_sync_state (device_id, state_json, last_sync_version, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                state.device_id().as_slice(),
                state_json,
                state.last_sync_version() as i64,
                now as i64,
            ],
        )?;
        Ok(())
    }

    /// Loads inter-device sync state for a specific device.
    pub fn load_device_sync_state(
        &self,
        device_id: &[u8; 32],
    ) -> Result<Option<InterDeviceSyncState>, StorageError> {
        let result = self.conn.query_row(
            "SELECT state_json FROM device_sync_state WHERE device_id = ?1",
            params![device_id.as_slice()],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(json) => {
                let state = InterDeviceSyncState::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Lists all inter-device sync states.
    pub fn list_device_sync_states(&self) -> Result<Vec<InterDeviceSyncState>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT state_json FROM device_sync_state")?;

        let states = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|json| InterDeviceSyncState::from_json(&json).ok())
            .collect();

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

    /// Saves the local version vector.
    pub fn save_version_vector(&self, vector: &VersionVector) -> Result<(), StorageError> {
        let vector_json = vector.to_json();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO version_vector (id, vector_json, updated_at)
             VALUES (1, ?1, ?2)",
            params![vector_json, now as i64,],
        )?;
        Ok(())
    }

    /// Loads the local version vector.
    pub fn load_version_vector(&self) -> Result<Option<VersionVector>, StorageError> {
        let result = self.conn.query_row(
            "SELECT vector_json FROM version_vector WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(json) => {
                let vector = VersionVector::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(vector))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}
