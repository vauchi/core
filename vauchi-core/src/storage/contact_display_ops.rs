// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage operations for contact display: nickname, custom avatar,
//! name variants, and display preferences (V43).

use rusqlite::params;

use super::{Storage, StorageError};

impl Storage {
    /// Saves an encrypted nickname for a contact.
    ///
    /// The caller passes plaintext bytes; this method encrypts with the storage
    /// encryption key before writing to the `nickname_encrypted` column.
    pub fn save_contact_nickname(
        &self,
        contact_id: &str,
        nickname_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(&self.encryption_key, nickname_bytes)
            .map_err(|e| StorageError::Encryption(format!("Encrypt nickname: {}", e)))?;
        let rows = self.conn.execute(
            "UPDATE contacts SET nickname_encrypted = ?1 WHERE id = ?2",
            params![encrypted, contact_id],
        )?;
        if rows == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }
        Ok(())
    }

    /// Loads and decrypts the nickname for a contact.
    ///
    /// Returns `None` if no nickname is stored, or `NotFound` if the contact
    /// does not exist.
    pub fn load_contact_nickname(&self, contact_id: &str) -> Result<Option<String>, StorageError> {
        let result = self.conn.query_row(
            "SELECT nickname_encrypted FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );
        match result {
            Ok(Some(encrypted)) => {
                let plain = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(format!("Decrypt nickname: {}", e)))?;
                let text = String::from_utf8(plain)
                    .map_err(|e| StorageError::Encryption(format!("Nickname not UTF-8: {}", e)))?;
                Ok(Some(text))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StorageError::NotFound("Contact not found".to_string()))
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Clears the nickname for a contact by setting the column to NULL.
    pub fn delete_contact_nickname(&self, contact_id: &str) -> Result<(), StorageError> {
        let rows = self.conn.execute(
            "UPDATE contacts SET nickname_encrypted = NULL WHERE id = ?1",
            params![contact_id],
        )?;
        if rows == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }
        Ok(())
    }
}
