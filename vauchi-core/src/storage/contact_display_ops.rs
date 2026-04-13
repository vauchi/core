// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage operations for contact display: nickname, custom avatar,
//! shared names/avatars, and display preferences (V43).

use rusqlite::params;

use super::{Storage, StorageError};
use crate::contact::display::{AvatarPreference, DisplayNamePreference, SharedAvatar, SharedName};

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

    /// Checks if a contact has a custom avatar without loading/decrypting it.
    pub fn has_contact_custom_avatar(&self, contact_id: &str) -> Result<bool, StorageError> {
        let result = self.conn.query_row(
            "SELECT custom_avatar_encrypted IS NOT NULL FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, bool>(0),
        );
        match result {
            Ok(has) => Ok(has),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StorageError::NotFound("Contact not found".to_string()))
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Shared Name Operations ===
    //
    // Encrypted with the storage key (self.encryption_key), not the contact's shared key.
    // Shared names are local-only data — the flat set received from the sender.

    /// Adds or updates a shared name for a contact.
    pub fn add_shared_name(
        &self,
        contact_id: &str,
        name: &str,
        is_primary: bool,
    ) -> Result<(), StorageError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();
        self.conn.execute(
            "INSERT INTO contact_shared_names (contact_id, name, is_primary, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(contact_id, name) DO UPDATE SET is_primary = ?3, updated_at = ?4",
            params![contact_id, name, is_primary as i32, now as i64],
        )?;
        Ok(())
    }

    /// Removes a shared name for a contact.
    pub fn remove_shared_name(&self, contact_id: &str, name: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM contact_shared_names WHERE contact_id = ?1 AND name = ?2",
            params![contact_id, name],
        )?;
        Ok(())
    }

    /// Lists all shared names for a contact (primary first).
    pub fn list_shared_names(&self, contact_id: &str) -> Result<Vec<SharedName>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT name, is_primary, updated_at
             FROM contact_shared_names
             WHERE contact_id = ?1
             ORDER BY is_primary DESC, name",
        )?;
        let rows = stmt.query_map(params![contact_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut names = Vec::new();
        for r in rows {
            let (name, is_primary, updated_at) = r?;
            names.push(SharedName {
                name,
                is_primary: is_primary != 0,
                updated_at: updated_at as u64,
            });
        }
        Ok(names)
    }

    // === Shared Avatar Operations ===

    /// Adds or updates a shared avatar for a contact.
    pub fn add_shared_avatar(
        &self,
        contact_id: &str,
        avatar_hash: &str,
        avatar_data: &[u8],
        is_primary: bool,
    ) -> Result<(), StorageError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();
        let encrypted = crate::crypto::encrypt(&self.encryption_key, avatar_data)
            .map_err(|e| StorageError::Encryption(format!("Encrypt shared avatar: {}", e)))?;
        self.conn.execute(
            "INSERT INTO contact_shared_avatars (contact_id, avatar_hash, avatar_encrypted, is_primary, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(contact_id, avatar_hash) DO UPDATE SET avatar_encrypted = ?3, is_primary = ?4, updated_at = ?5",
            params![contact_id, avatar_hash, encrypted, is_primary as i32, now as i64],
        )?;
        Ok(())
    }

    /// Removes a shared avatar for a contact.
    pub fn remove_shared_avatar(
        &self,
        contact_id: &str,
        avatar_hash: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM contact_shared_avatars WHERE contact_id = ?1 AND avatar_hash = ?2",
            params![contact_id, avatar_hash],
        )?;
        Ok(())
    }

    /// Lists all shared avatars for a contact (primary first).
    pub fn list_shared_avatars(&self, contact_id: &str) -> Result<Vec<SharedAvatar>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT avatar_hash, is_primary, updated_at
             FROM contact_shared_avatars
             WHERE contact_id = ?1
             ORDER BY is_primary DESC",
        )?;
        let rows = stmt.query_map(params![contact_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut avatars = Vec::new();
        for r in rows {
            let (hash, is_primary, updated_at) = r?;
            avatars.push(SharedAvatar {
                avatar_hash: hash,
                is_primary: is_primary != 0,
                updated_at: updated_at as u64,
            });
        }
        Ok(avatars)
    }

    // === Display Preference Operations ===

    /// Saves the display name preference for a contact.
    pub fn save_display_name_preference(
        &self,
        contact_id: &str,
        pref: &DisplayNamePreference,
    ) -> Result<(), StorageError> {
        let json =
            serde_json::to_string(pref).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let rows = self.conn.execute(
            "UPDATE contacts SET display_name_preference = ?1 WHERE id = ?2",
            params![json, contact_id],
        )?;
        if rows == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }
        Ok(())
    }

    /// Saves the avatar preference for a contact.
    pub fn save_avatar_preference(
        &self,
        contact_id: &str,
        pref: &AvatarPreference,
    ) -> Result<(), StorageError> {
        let json =
            serde_json::to_string(pref).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let rows = self.conn.execute(
            "UPDATE contacts SET avatar_preference = ?1 WHERE id = ?2",
            params![json, contact_id],
        )?;
        if rows == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }
        Ok(())
    }

    /// Loads both display preferences for a contact.
    pub fn load_display_preferences(
        &self,
        contact_id: &str,
    ) -> Result<(DisplayNamePreference, AvatarPreference), StorageError> {
        let result = self.conn.query_row(
            "SELECT display_name_preference, avatar_preference FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        );
        match result {
            Ok((name_json, avatar_json)) => {
                let name_pref: DisplayNamePreference =
                    serde_json::from_str(&name_json).unwrap_or_default();
                let avatar_pref: AvatarPreference =
                    serde_json::from_str(&avatar_json).unwrap_or_default();
                Ok((name_pref, avatar_pref))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StorageError::NotFound("Contact not found".to_string()))
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}
