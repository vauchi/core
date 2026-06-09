// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact domain persistence view — display ops (impl ContactStore).

use rusqlite::params;
use std::collections::{HashMap, HashSet};

use super::super::StorageError;
use super::ContactStore;
use crate::contact::display::{AvatarPreference, DisplayNamePreference, SharedAvatar, SharedName};

impl ContactStore<'_> {
    /// Saves an encrypted nickname for a contact.
    ///
    /// The caller passes plaintext bytes; this method encrypts with the storage
    /// encryption key before writing to the `nickname_encrypted` column.
    pub fn save_contact_nickname(
        &self,
        contact_id: &str,
        nickname_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, nickname_bytes)
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
                let plain = crate::crypto::decrypt(self.key, &encrypted)
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
    /// Saves an encrypted custom avatar for a contact.
    pub fn save_contact_custom_avatar(
        &self,
        contact_id: &str,
        avatar_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, avatar_bytes)
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
                let plain = crate::crypto::decrypt(self.key, &encrypted).map_err(|e| {
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
    //
    // Encrypted with the storage key (self.encryption_key), not the contact's shared key.
    // Shared names are local-only data — the flat set received from the sender.

    /// Adds or updates a shared name for a contact.
    ///
    /// When `is_primary` is true, clears the previous primary first
    /// to maintain the exactly-one-primary invariant.
    pub fn add_shared_name(
        &self,
        contact_id: &str,
        name: &str,
        is_primary: bool,
    ) -> Result<(), StorageError> {
        let now = self.now_secs();
        if is_primary {
            self.conn.execute(
                "UPDATE contact_shared_names SET is_primary = 0 WHERE contact_id = ?1 AND is_primary = 1",
                params![contact_id],
            )?;
        }
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
    /// Adds or updates a shared avatar for a contact.
    ///
    /// When `is_primary` is true, clears the previous primary first
    /// to maintain the exactly-one-primary invariant.
    pub fn add_shared_avatar(
        &self,
        contact_id: &str,
        avatar_hash: &str,
        avatar_data: &[u8],
        is_primary: bool,
    ) -> Result<(), StorageError> {
        let now = self.now_secs();
        if is_primary {
            self.conn.execute(
                "UPDATE contact_shared_avatars SET is_primary = 0 WHERE contact_id = ?1 AND is_primary = 1",
                params![contact_id],
            )?;
        }
        let encrypted = crate::crypto::encrypt(self.key, avatar_data)
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
    // === Batch Operations (N+1 prevention) ===

    /// Batch-load shared names for multiple contacts in a single query.
    ///
    /// Returns a map of contact_id → Vec<SharedName>. Contacts with no shared
    /// names are absent from the map (callers should use `.unwrap_or_default()`).
    pub fn batch_shared_names(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashMap<String, Vec<SharedName>>, StorageError> {
        if contact_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> =
            (1..=contact_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT contact_id, name, is_primary, updated_at
             FROM contact_shared_names
             WHERE contact_id IN ({})
             ORDER BY contact_id, is_primary DESC, name",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(contact_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i32>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut map: HashMap<String, Vec<SharedName>> = HashMap::new();
        for r in rows {
            let (cid, name, is_primary, updated_at) = r?;
            map.entry(cid).or_default().push(SharedName {
                name,
                is_primary: is_primary != 0,
                updated_at: updated_at as u64,
            });
        }
        Ok(map)
    }
    /// Batch-load nicknames for multiple contacts in a single query.
    ///
    /// Returns a map of contact_id → decrypted nickname string. Contacts with
    /// no nickname (NULL) are absent from the map.
    pub fn batch_nicknames(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashMap<String, String>, StorageError> {
        if contact_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> =
            (1..=contact_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT id, nickname_encrypted
             FROM contacts
             WHERE id IN ({}) AND nickname_encrypted IS NOT NULL",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(contact_ids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (cid, encrypted) = r?;
            let plain = crate::crypto::decrypt(self.key, &encrypted)
                .map_err(|e| StorageError::Encryption(format!("Decrypt nickname: {}", e)))?;
            let text = String::from_utf8(plain)
                .map_err(|e| StorageError::Encryption(format!("Nickname not UTF-8: {}", e)))?;
            map.insert(cid, text);
        }
        Ok(map)
    }
    /// Batch-load display preferences for multiple contacts in a single query.
    ///
    /// Returns a map of contact_id → (DisplayNamePreference, AvatarPreference).
    /// Contacts absent from the result should fall back to their defaults.
    pub fn batch_display_preferences(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashMap<String, (DisplayNamePreference, AvatarPreference)>, StorageError> {
        if contact_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders: Vec<String> =
            (1..=contact_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT id, display_name_preference, avatar_preference
             FROM contacts
             WHERE id IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(contact_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (cid, name_json, avatar_json) = r?;
            let name_pref: DisplayNamePreference =
                serde_json::from_str(&name_json).unwrap_or_default();
            let avatar_pref: AvatarPreference =
                serde_json::from_str(&avatar_json).unwrap_or_default();
            map.insert(cid, (name_pref, avatar_pref));
        }
        Ok(map)
    }
    /// Batch-check which contacts have custom avatars.
    ///
    /// Returns the set of contact_ids that have a non-NULL custom avatar.
    /// Absence from the set means no custom avatar.
    pub fn batch_has_custom_avatar(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashSet<String>, StorageError> {
        if contact_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let placeholders: Vec<String> =
            (1..=contact_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT id FROM contacts
             WHERE id IN ({}) AND custom_avatar_encrypted IS NOT NULL",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(contact_ids.iter()), |row| {
            row.get::<_, String>(0)
        })?;
        let mut set = HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }
}
