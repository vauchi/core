// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity storage operations.

use rusqlite::params;

use super::{Storage, StorageError};

impl Storage {
    // === Identity Operations ===

    /// Saves identity backup data (encrypted).
    pub fn save_identity(
        &self,
        backup_data: &[u8],
        display_name: &str,
    ) -> Result<(), StorageError> {
        // Encrypt the backup data
        let encrypted = crate::crypto::encrypt(&self.encryption_key, backup_data)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO identity (id, backup_data_encrypted, display_name, created_at) VALUES (1, ?1, ?2, ?3)",
            params![encrypted, display_name, now as i64],
        )?;

        Ok(())
    }

    /// Loads identity backup data (decrypted).
    /// Returns (backup_data, display_name) if found.
    pub fn load_identity(&self) -> Result<Option<(Vec<u8>, String)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT backup_data_encrypted, display_name FROM identity WHERE id = 1",
            [],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
        );

        match result {
            Ok((encrypted, display_name)) => {
                let backup_data = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                Ok(Some((backup_data, display_name)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Checks if identity exists.
    pub fn has_identity(&self) -> Result<bool, StorageError> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM identity WHERE id = 1", [], |row| {
                    row.get(0)
                })?;
        Ok(count > 0)
    }
}
