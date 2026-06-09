// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device domain persistence view.
//!
//! `DeviceStore` owns this device's identity record (`device_info`) and the
//! multi-device registry (`device_registry`). Inter-device *sync* state lives
//! in [`SyncStore`](super::SyncStore) per decision (a) of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`; replay-nonce defense
//! (ADR-029) is still on `Storage` pending its own exchange/security store.

use std::sync::Arc;

use rusqlite::{Connection, params};

use super::super::{Storage, StorageError};
use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use crate::identity::device::DeviceRegistry;

/// Scoped persistence view for the device domain: this device's info record and
/// the device registry.
pub struct DeviceStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the device domain.
    ///
    /// The returned [`DeviceStore`] borrows this storage's connection,
    /// encryption key, and clock; it cannot outlive `self` and reaches only
    /// the device-owned tables.
    pub fn device(&self) -> DeviceStore<'_> {
        DeviceStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl DeviceStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }

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
        let encrypted = crate::crypto::encrypt(self.key, &json_bytes)
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
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
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

    /// Deletes this device's info row (used during device reset / unlink).
    pub fn clear_device_info(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM device_info", [])?;
        Ok(())
    }

    /// Saves the device registry (encrypted).
    pub fn save_device_registry(&self, registry: &DeviceRegistry) -> Result<(), StorageError> {
        let registry_json = serde_json::to_string(registry)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let registry_encrypted = crate::crypto::encrypt(self.key, registry_json.as_bytes())
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
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
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
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
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
}
