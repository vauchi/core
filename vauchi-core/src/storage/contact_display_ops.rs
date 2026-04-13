// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage operations for contact display: nickname, custom avatar,
//! name variants, and display preferences (V43).

use rusqlite::params;

use super::{Storage, StorageError};
use crate::contact::display::NameVariant;

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

    // === Custom Avatar Operations ===

    /// Saves an encrypted custom avatar for a contact.
    pub fn save_contact_custom_avatar(
        &self,
        contact_id: &str,
        avatar_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(&self.encryption_key, avatar_bytes)
            .map_err(|e| StorageError::Encryption(format!("Encrypt custom avatar: {}", e)))?;
        let rows = self.conn.execute(
            "UPDATE contacts SET custom_avatar_encrypted = ?1 WHERE id = ?2",
            params![encrypted, contact_id],
        )?;
        if rows == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }
        Ok(())
    }

    /// Loads and decrypts the custom avatar for a contact.
    pub fn load_contact_custom_avatar(
        &self,
        contact_id: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let result = self.conn.query_row(
            "SELECT custom_avatar_encrypted FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );
        match result {
            Ok(Some(encrypted)) => {
                let plain =
                    crate::crypto::decrypt(&self.encryption_key, &encrypted).map_err(|e| {
                        StorageError::Encryption(format!("Decrypt custom avatar: {}", e))
                    })?;
                Ok(Some(plain))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StorageError::NotFound("Contact not found".to_string()))
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Clears the custom avatar for a contact.
    pub fn delete_contact_custom_avatar(&self, contact_id: &str) -> Result<(), StorageError> {
        let rows = self.conn.execute(
            "UPDATE contacts SET custom_avatar_encrypted = NULL WHERE id = ?1",
            params![contact_id],
        )?;
        if rows == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }
        Ok(())
    }

    // === Name Variant Operations ===

    /// Upserts a name variant for a contact (called by sync layer on card updates).
    pub fn upsert_name_variant(
        &self,
        contact_id: &str,
        source_label: &str,
        name: &str,
        avatar: Option<&[u8]>,
    ) -> Result<(), StorageError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        let avatar_encrypted =
            match avatar {
                Some(data) => Some(crate::crypto::encrypt(&self.encryption_key, data).map_err(
                    |e| StorageError::Encryption(format!("Encrypt variant avatar: {}", e)),
                )?),
                None => None,
            };

        self.conn.execute(
            "INSERT INTO contact_name_variants (contact_id, source_label, name, avatar_encrypted, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(contact_id, source_label)
             DO UPDATE SET name = ?3, avatar_encrypted = ?4, updated_at = ?5",
            params![contact_id, source_label, name, avatar_encrypted, now as i64],
        )?;
        Ok(())
    }

    /// Lists all name variants for a contact.
    pub fn list_name_variants(&self, contact_id: &str) -> Result<Vec<NameVariant>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT source_label, name, avatar_encrypted, updated_at
             FROM contact_name_variants
             WHERE contact_id = ?1
             ORDER BY source_label",
        )?;
        let rows = stmt.query_map(params![contact_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<Vec<u8>>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        let mut variants = Vec::new();
        for row_result in rows {
            let (source_label, name, avatar_enc, updated_at) = row_result?;
            variants.push(NameVariant {
                source_label,
                name,
                has_avatar: avatar_enc.is_some(),
                updated_at: updated_at as u64,
            });
        }
        Ok(variants)
    }
}
