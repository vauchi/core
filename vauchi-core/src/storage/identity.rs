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

    /// Deletes the persisted identity row.
    ///
    /// Used by `Vauchi::perform_emergency_wipe` to clear identity from
    /// storage. After this call, `has_identity()` returns `false` and
    /// `load_identity()` returns `None`. Idempotent — succeeds even if
    /// no row exists.
    pub fn delete_identity(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM identity WHERE id = 1", [])?;
        Ok(())
    }

    // === App Password / Duress PIN Operations ===

    /// Saves the app password hash and salt to the identity table.
    ///
    /// The hash is encrypted with the storage key before persisting.
    pub fn save_app_password(&self, hash: &[u8; 32], salt: &[u8; 16]) -> Result<(), StorageError> {
        let encrypted_hash = crate::crypto::encrypt(&self.encryption_key, hash)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "UPDATE identity SET password_hash_encrypted = ?1, password_salt = ?2 WHERE id = 1",
            params![encrypted_hash, salt.as_slice()],
        )?;

        Ok(())
    }

    /// Saves the duress password hash and salt, and enables duress mode.
    ///
    /// The hash is encrypted with the storage key before persisting.
    pub fn save_duress_password(
        &self,
        hash: &[u8; 32],
        salt: &[u8; 16],
    ) -> Result<(), StorageError> {
        let encrypted_hash = crate::crypto::encrypt(&self.encryption_key, hash)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "UPDATE identity SET duress_hash_encrypted = ?1, duress_salt = ?2, duress_enabled = 1 WHERE id = 1",
            params![encrypted_hash, salt.as_slice(), ],
        )?;

        Ok(())
    }

    /// Loads the password configuration from the identity table.
    ///
    /// Returns `None` if no password has been set (all password columns are NULL).
    pub fn load_password_config(
        &self,
    ) -> Result<Option<crate::api::app_password::AppPasswordConfig>, StorageError> {
        let result = self.conn.query_row(
            "SELECT password_hash_encrypted, password_salt, duress_hash_encrypted, duress_salt, duress_enabled FROM identity WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<Vec<u8>>>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                    row.get::<_, i32>(4)?,
                ))
            },
        );

        match result {
            Ok((
                Some(hash_enc),
                Some(salt_bytes),
                duress_hash_enc,
                duress_salt_bytes,
                duress_enabled,
            )) => {
                // Decrypt the password hash
                let hash_bytes = crate::crypto::decrypt(&self.encryption_key, &hash_enc)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;

                let password_hash: [u8; 32] = hash_bytes.try_into().map_err(|_| {
                    StorageError::InvalidData("password hash has invalid length".into())
                })?;

                let password_salt: [u8; 16] = salt_bytes.try_into().map_err(|_| {
                    StorageError::InvalidData("password salt has invalid length".into())
                })?;

                // Decrypt duress hash if present
                let duress_hash = if let Some(enc) = duress_hash_enc {
                    let bytes = crate::crypto::decrypt(&self.encryption_key, &enc)
                        .map_err(|e| StorageError::Encryption(e.to_string()))?;
                    let arr: [u8; 32] = bytes.try_into().map_err(|_| {
                        StorageError::InvalidData("duress hash has invalid length".into())
                    })?;
                    Some(arr)
                } else {
                    None
                };

                let duress_salt = if let Some(bytes) = duress_salt_bytes {
                    let arr: [u8; 16] = bytes.try_into().map_err(|_| {
                        StorageError::InvalidData("duress salt has invalid length".into())
                    })?;
                    Some(arr)
                } else {
                    None
                };

                Ok(Some(
                    crate::api::app_password::AppPasswordConfig::from_stored(
                        password_hash,
                        password_salt,
                        duress_hash,
                        duress_salt,
                        duress_enabled != 0,
                    ),
                ))
            }
            Ok((None, _, _, _, _)) | Ok((_, None, _, _, _)) => {
                // No password set
                Ok(None)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Disables duress mode and clears duress hash/salt.
    pub fn disable_duress(&self) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE identity SET duress_hash_encrypted = NULL, duress_salt = NULL, duress_enabled = 0 WHERE id = 1",
            [],
        )?;

        Ok(())
    }
}
