// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage operations for visibility labels.
//!
//! Provides CRUD operations for persisting labels and per-contact overrides.

use std::collections::{HashMap, HashSet};

use crate::contact::{LabelManager, VisibilityLabel};

use super::{Storage, StorageError};

impl Storage {
    /// Saves a visibility label to storage (encrypted).
    ///
    /// Label name is encrypted at rest with HMAC for lookups (#128).
    pub fn save_label(&self, label: &VisibilityLabel) -> Result<(), StorageError> {
        let contacts_json = serde_json::to_string(label.contacts())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let fields_json = serde_json::to_string(label.visible_fields())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let contacts_encrypted =
            crate::crypto::encrypt(&self.encryption_key, contacts_json.as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let fields_encrypted = crate::crypto::encrypt(&self.encryption_key, fields_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Encrypt label name and compute HMAC for lookups (#128)
        let name_encrypted = crate::crypto::encrypt(&self.encryption_key, label.name().as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let name_hmac =
            self.compute_lookup_hmac(b"Vauchi_Label_Name_HMAC_v1", label.name().as_bytes());

        // Encrypt display_name_override if present, NULL otherwise
        let display_name_override_encrypted: Option<Vec<u8>> = match label.display_name_override() {
            Some(override_name) => {
                let encrypted =
                    crate::crypto::encrypt(&self.encryption_key, override_name.as_bytes())
                        .map_err(|e| StorageError::Encryption(e.to_string()))?;
                Some(encrypted)
            }
            None => None,
        };

        // Store label id in plaintext `name` column to satisfy UNIQUE constraint
        // without leaking the actual name (which is in name_encrypted).
        self.conn.execute(
            "INSERT OR REPLACE INTO visibility_labels
             (id, name, name_encrypted, name_hmac, contacts_json, visible_fields_json, contacts_json_encrypted, visible_fields_json_encrypted, created_at, modified_at, display_name_override_encrypted)
             VALUES (?1, ?2, ?3, ?4, '[]', '[]', ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                label.id(),
                label.id(),
                name_encrypted,
                name_hmac,
                contacts_encrypted,
                fields_encrypted,
                label.created_at() as i64,
                label.modified_at() as i64,
                display_name_override_encrypted,
            ],
        )?;

        Ok(())
    }

    /// Loads a visibility label by ID (decrypted).
    pub fn load_label(&self, label_id: &str) -> Result<VisibilityLabel, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, contacts_json, visible_fields_json, created_at, modified_at, contacts_json_encrypted, visible_fields_json_encrypted, name_encrypted, display_name_override_encrypted
             FROM visibility_labels WHERE id = ?1",
        )?;

        let label = stmt.query_row([label_id], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let contacts_json: String = row.get(2)?;
            let fields_json: String = row.get(3)?;
            let created_at: i64 = row.get(4)?;
            let modified_at: i64 = row.get(5)?;
            let contacts_encrypted: Option<Vec<u8>> = row.get(6)?;
            let fields_encrypted: Option<Vec<u8>> = row.get(7)?;
            let name_encrypted: Option<Vec<u8>> = row.get(8)?;
            let display_name_override_encrypted: Option<Vec<u8>> = row.get(9)?;

            Ok((
                id,
                name,
                contacts_json,
                fields_json,
                created_at,
                modified_at,
                contacts_encrypted,
                fields_encrypted,
                name_encrypted,
                display_name_override_encrypted,
            ))
        })?;

        // Decrypt label name (#128): prefer encrypted, fall back to plaintext
        let name = self.decrypt_or_fallback(label.8.as_deref(), &label.1)?;

        let contacts_json = self.decrypt_or_fallback(label.6.as_deref(), &label.2)?;
        let fields_json = self.decrypt_or_fallback(label.7.as_deref(), &label.3)?;

        // Decrypt display_name_override if present
        let display_name_override = self.decrypt_optional_blob(label.9.as_deref())?;

        let contacts: HashSet<String> = serde_json::from_str(&contacts_json)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let visible_fields: HashSet<String> = serde_json::from_str(&fields_json)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        Ok(VisibilityLabel::from_storage(
            label.0,
            name,
            contacts,
            visible_fields,
            display_name_override,
            label.4 as u64,
            label.5 as u64,
        ))
    }

    /// Loads all visibility labels (decrypted).
    ///
    /// Labels are sorted by decrypted name in Rust since encrypted names
    /// cannot be sorted in SQL (#128).
    pub fn load_all_labels(&self) -> Result<Vec<VisibilityLabel>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, contacts_json, visible_fields_json, created_at, modified_at, contacts_json_encrypted, visible_fields_json_encrypted, name_encrypted, display_name_override_encrypted
             FROM visibility_labels",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let contacts_json: String = row.get(2)?;
            let fields_json: String = row.get(3)?;
            let created_at: i64 = row.get(4)?;
            let modified_at: i64 = row.get(5)?;
            let contacts_encrypted: Option<Vec<u8>> = row.get(6)?;
            let fields_encrypted: Option<Vec<u8>> = row.get(7)?;
            let name_encrypted: Option<Vec<u8>> = row.get(8)?;
            let display_name_override_encrypted: Option<Vec<u8>> = row.get(9)?;

            Ok((
                id,
                name,
                contacts_json,
                fields_json,
                created_at,
                modified_at,
                contacts_encrypted,
                fields_encrypted,
                name_encrypted,
                display_name_override_encrypted,
            ))
        })?;

        let mut labels = Vec::new();
        for row_result in rows {
            let row = row_result?;
            let name = self.decrypt_or_fallback(row.8.as_deref(), &row.1)?;
            let contacts_json = self.decrypt_or_fallback(row.6.as_deref(), &row.2)?;
            let fields_json = self.decrypt_or_fallback(row.7.as_deref(), &row.3)?;
            let display_name_override = self.decrypt_optional_blob(row.9.as_deref())?;

            let contacts: HashSet<String> = serde_json::from_str(&contacts_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            let visible_fields: HashSet<String> = serde_json::from_str(&fields_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;

            labels.push(VisibilityLabel::from_storage(
                row.0,
                name,
                contacts,
                visible_fields,
                display_name_override,
                row.4 as u64,
                row.5 as u64,
            ));
        }

        // Sort by decrypted name (can't ORDER BY in SQL with encrypted names)
        labels.sort_by(|a, b| a.name().cmp(b.name()));

        Ok(labels)
    }

    /// Decrypts an optional encrypted blob, returning None if the blob is NULL.
    ///
    /// Used for nullable encrypted columns like `display_name_override_encrypted`.
    fn decrypt_optional_blob(
        &self,
        encrypted: Option<&[u8]>,
    ) -> Result<Option<String>, StorageError> {
        match encrypted {
            Some(enc) if !enc.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, enc)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let s = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(s))
            }
            _ => Ok(None),
        }
    }

    /// Decrypts an encrypted blob, falling back to plaintext only for
    /// pre-migration data (#156).
    fn decrypt_or_fallback(
        &self,
        encrypted: Option<&[u8]>,
        plaintext_fallback: &str,
    ) -> Result<String, StorageError> {
        if let Some(enc) = encrypted {
            if !enc.is_empty() {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, enc)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                return String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()));
            }
        }
        // Plaintext fallback is only valid for pre-migration data where
        // the plaintext columns contained real data. Post-migration, plaintext
        // is always '[]'. Return an error if the fallback itself is empty/default
        // so callers don't silently lose data.
        if plaintext_fallback == "[]" {
            return Err(StorageError::Encryption(
                "encrypted column is empty and plaintext fallback contains no data".to_string(),
            ));
        }
        Ok(plaintext_fallback.to_string())
    }

    /// Deletes a visibility label.
    pub fn delete_label(&self, label_id: &str) -> Result<(), StorageError> {
        let changes = self
            .conn
            .execute("DELETE FROM visibility_labels WHERE id = ?1", [label_id])?;

        if changes == 0 {
            return Err(StorageError::NotFound(format!("Label: {}", label_id)));
        }

        Ok(())
    }

    /// Saves a per-contact visibility override.
    pub fn save_contact_override(
        &self,
        contact_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO contact_visibility_overrides
             (contact_id, field_id, is_visible)
             VALUES (?1, ?2, ?3)",
            (contact_id, field_id, is_visible as i32),
        )?;

        Ok(())
    }

    /// Deletes a per-contact visibility override.
    pub fn delete_contact_override(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM contact_visibility_overrides
             WHERE contact_id = ?1 AND field_id = ?2",
            (contact_id, field_id),
        )?;

        Ok(())
    }

    /// Loads all per-contact overrides for a contact.
    pub fn load_contact_overrides(
        &self,
        contact_id: &str,
    ) -> Result<HashMap<String, bool>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT field_id, is_visible FROM contact_visibility_overrides
             WHERE contact_id = ?1",
        )?;

        let rows = stmt.query_map([contact_id], |row| {
            let field_id: String = row.get(0)?;
            let is_visible: i32 = row.get(1)?;
            Ok((field_id, is_visible != 0))
        })?;

        let mut overrides = HashMap::new();
        for row_result in rows {
            let (field_id, is_visible) = row_result?;
            overrides.insert(field_id, is_visible);
        }

        Ok(overrides)
    }

    /// Loads all per-contact overrides (all contacts).
    pub fn load_all_contact_overrides(
        &self,
    ) -> Result<HashMap<String, HashMap<String, bool>>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, field_id, is_visible FROM contact_visibility_overrides")?;

        let rows = stmt.query_map([], |row| {
            let contact_id: String = row.get(0)?;
            let field_id: String = row.get(1)?;
            let is_visible: i32 = row.get(2)?;
            Ok((contact_id, field_id, is_visible != 0))
        })?;

        let mut all_overrides: HashMap<String, HashMap<String, bool>> = HashMap::new();
        for row_result in rows {
            let (contact_id, field_id, is_visible) = row_result?;
            all_overrides
                .entry(contact_id)
                .or_default()
                .insert(field_id, is_visible);
        }

        Ok(all_overrides)
    }

    /// Deletes all per-contact overrides for a contact.
    pub fn delete_all_contact_overrides(&self, contact_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM contact_visibility_overrides WHERE contact_id = ?1",
            [contact_id],
        )?;

        Ok(())
    }

    /// Saves a complete LabelManager to storage.
    ///
    /// This saves all labels and all per-contact overrides.
    pub fn save_label_manager(&self, manager: &LabelManager) -> Result<(), StorageError> {
        // Save all labels
        for label in manager.all_labels() {
            self.save_label(label)?;
        }

        // Note: Per-contact overrides are saved individually as they're set
        // This method primarily saves the label state

        Ok(())
    }

    /// Loads a complete LabelManager from storage.
    ///
    /// This loads all labels and all per-contact overrides.
    pub fn load_label_manager(&self) -> Result<LabelManager, StorageError> {
        let labels = self.load_all_labels()?;
        let overrides = self.load_all_contact_overrides()?;

        let mut manager = LabelManager::new();

        // Reconstruct labels preserving stored IDs and all fields
        for label in labels {
            manager.insert_loaded_label(label);
        }

        // Add per-contact overrides
        for (contact_id, field_overrides) in overrides {
            for (field_id, is_visible) in field_overrides {
                manager.set_contact_override(&contact_id, &field_id, is_visible);
            }
        }

        Ok(manager)
    }

    /// Creates a label in storage.
    ///
    /// Returns the created label.
    pub fn create_label(&self, name: &str) -> Result<VisibilityLabel, StorageError> {
        // Validate name
        let name = name.trim();
        if name.is_empty() {
            return Err(StorageError::Serialization(
                "Label name cannot be empty".to_string(),
            ));
        }
        if name.chars().count() > 50 {
            return Err(StorageError::Serialization(
                "Label name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate via HMAC lookup (#128)
        let name_hmac = self.compute_lookup_hmac(b"Vauchi_Label_Name_HMAC_v1", name.as_bytes());
        let existing = self.conn.query_row(
            "SELECT COUNT(*) FROM visibility_labels WHERE name_hmac = ?1",
            [&name_hmac],
            |row| row.get::<_, i32>(0),
        )?;

        if existing > 0 {
            return Err(StorageError::AlreadyExists(format!("Label: {}", name)));
        }

        // Check max labels
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM visibility_labels", [], |row| {
                row.get::<_, i32>(0)
            })?;

        if count >= crate::contact::MAX_LABELS as i32 {
            return Err(StorageError::Serialization(format!(
                "Maximum number of labels reached ({})",
                crate::contact::MAX_LABELS
            )));
        }

        // Create and save
        let label = VisibilityLabel::new(name);
        self.save_label(&label)?;

        Ok(label)
    }

    /// Renames a label in storage.
    pub fn rename_label(&self, label_id: &str, new_name: &str) -> Result<(), StorageError> {
        let new_name = new_name.trim();

        // Validate new name
        if new_name.is_empty() {
            return Err(StorageError::Serialization(
                "Label name cannot be empty".to_string(),
            ));
        }
        if new_name.chars().count() > 50 {
            return Err(StorageError::Serialization(
                "Label name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate via HMAC lookup (#128)
        let new_name_hmac =
            self.compute_lookup_hmac(b"Vauchi_Label_Name_HMAC_v1", new_name.as_bytes());
        let existing = self.conn.query_row(
            "SELECT COUNT(*) FROM visibility_labels WHERE name_hmac = ?1 AND id != ?2",
            rusqlite::params![new_name_hmac, label_id],
            |row| row.get::<_, i32>(0),
        )?;

        if existing > 0 {
            return Err(StorageError::AlreadyExists(format!("Label: {}", new_name)));
        }

        // Encrypt new name (#128)
        let name_encrypted = crate::crypto::encrypt(&self.encryption_key, new_name.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Update
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let changes = self.conn.execute(
            "UPDATE visibility_labels SET name_encrypted = ?1, name_hmac = ?2, modified_at = ?3 WHERE id = ?4",
            rusqlite::params![name_encrypted, new_name_hmac, now as i64, label_id],
        )?;

        if changes == 0 {
            return Err(StorageError::NotFound(format!("Label: {}", label_id)));
        }

        Ok(())
    }

    /// Adds a contact to a label in storage.
    pub fn add_contact_to_label(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> Result<(), StorageError> {
        // Load the label
        let mut label = self.load_label(label_id)?;

        // Add the contact
        label.add_contact(contact_id);

        // Save back
        self.save_label(&label)?;

        Ok(())
    }

    /// Removes a contact from a label in storage.
    pub fn remove_contact_from_label(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> Result<(), StorageError> {
        // Load the label
        let mut label = self.load_label(label_id)?;

        // Remove the contact
        label.remove_contact(contact_id);

        // Save back
        self.save_label(&label)?;

        Ok(())
    }

    /// Removes a contact from all labels in storage.
    ///
    /// Call this when deleting a contact.
    pub fn remove_contact_from_all_labels(&self, contact_id: &str) -> Result<(), StorageError> {
        let labels = self.load_all_labels()?;

        for mut label in labels {
            if label.contains_contact(contact_id) {
                label.remove_contact(contact_id);
                self.save_label(&label)?;
            }
        }

        // Also remove per-contact overrides
        self.delete_all_contact_overrides(contact_id)?;

        Ok(())
    }

    /// Sets a field's visibility for a label in storage.
    pub fn set_label_field_visibility(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> Result<(), StorageError> {
        // Load the label
        let mut label = self.load_label(label_id)?;

        // Update visibility
        if is_visible {
            label.add_visible_field(field_id);
        } else {
            label.remove_visible_field(field_id);
        }

        // Save back
        self.save_label(&label)?;

        Ok(())
    }

    /// Gets all labels that contain a specific contact.
    pub fn get_labels_for_contact(
        &self,
        contact_id: &str,
    ) -> Result<Vec<VisibilityLabel>, StorageError> {
        let labels = self.load_all_labels()?;

        Ok(labels
            .into_iter()
            .filter(|l| l.contains_contact(contact_id))
            .collect())
    }
}

// INLINE_TEST_REQUIRED: tests access private storage internals and require in-memory DB setup
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SymmetricKey;

    fn test_storage() -> Storage {
        let key = SymmetricKey::generate();
        Storage::in_memory(key).unwrap()
    }

    #[test]
    fn test_save_and_load_label() {
        let storage = test_storage();

        let mut label = VisibilityLabel::new("Family");
        label.add_contact("alice-id");
        label.add_contact("bob-id");
        label.add_visible_field("phone");
        label.add_visible_field("address");

        storage.save_label(&label).unwrap();

        let loaded = storage.load_label(label.id()).unwrap();

        assert_eq!(loaded.name(), "Family");
        assert!(loaded.contains_contact("alice-id"));
        assert!(loaded.contains_contact("bob-id"));
        assert!(loaded.is_field_visible("phone"));
        assert!(loaded.is_field_visible("address"));
    }

    #[test]
    fn test_load_all_labels() {
        let storage = test_storage();

        let label1 = VisibilityLabel::new("Family");
        let label2 = VisibilityLabel::new("Friends");
        let label3 = VisibilityLabel::new("Work");

        storage.save_label(&label1).unwrap();
        storage.save_label(&label2).unwrap();
        storage.save_label(&label3).unwrap();

        let labels = storage.load_all_labels().unwrap();

        assert_eq!(labels.len(), 3);
        // Ordered by name
        assert_eq!(labels[0].name(), "Family");
        assert_eq!(labels[1].name(), "Friends");
        assert_eq!(labels[2].name(), "Work");
    }

    #[test]
    fn test_delete_label() {
        let storage = test_storage();

        let label = VisibilityLabel::new("Temporary");
        storage.save_label(&label).unwrap();

        storage.delete_label(label.id()).unwrap();

        let result = storage.load_label(label.id());
        assert!(result.is_err());
    }

    #[test]
    fn test_contact_overrides() {
        let storage = test_storage();

        storage
            .save_contact_override("alice-id", "phone", true)
            .unwrap();
        storage
            .save_contact_override("alice-id", "address", false)
            .unwrap();
        storage
            .save_contact_override("bob-id", "email", true)
            .unwrap();

        let alice_overrides = storage.load_contact_overrides("alice-id").unwrap();
        assert_eq!(alice_overrides.len(), 2);
        assert_eq!(alice_overrides.get("phone"), Some(&true));
        assert_eq!(alice_overrides.get("address"), Some(&false));

        let bob_overrides = storage.load_contact_overrides("bob-id").unwrap();
        assert_eq!(bob_overrides.len(), 1);
        assert_eq!(bob_overrides.get("email"), Some(&true));
    }

    #[test]
    fn test_delete_contact_override() {
        let storage = test_storage();

        storage
            .save_contact_override("alice-id", "phone", true)
            .unwrap();
        storage
            .delete_contact_override("alice-id", "phone")
            .unwrap();

        let overrides = storage.load_contact_overrides("alice-id").unwrap();
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_delete_all_contact_overrides() {
        let storage = test_storage();

        storage
            .save_contact_override("alice-id", "phone", true)
            .unwrap();
        storage
            .save_contact_override("alice-id", "address", false)
            .unwrap();

        storage.delete_all_contact_overrides("alice-id").unwrap();

        let overrides = storage.load_contact_overrides("alice-id").unwrap();
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_create_label() {
        let storage = test_storage();

        let label = storage.create_label("New Label").unwrap();
        assert_eq!(label.name(), "New Label");

        // Should be persisted
        let loaded = storage.load_label(label.id()).unwrap();
        assert_eq!(loaded.name(), "New Label");
    }

    #[test]
    fn test_create_duplicate_label() {
        let storage = test_storage();

        storage.create_label("Unique").unwrap();
        let result = storage.create_label("Unique");

        assert!(matches!(result, Err(StorageError::AlreadyExists(_))));
    }

    #[test]
    fn test_rename_label() {
        let storage = test_storage();

        let label = storage.create_label("Old Name").unwrap();
        storage.rename_label(label.id(), "New Name").unwrap();

        let loaded = storage.load_label(label.id()).unwrap();
        assert_eq!(loaded.name(), "New Name");
    }

    #[test]
    fn test_add_remove_contact_from_label() {
        let storage = test_storage();

        let label = storage.create_label("Test").unwrap();
        storage
            .add_contact_to_label(label.id(), "alice-id")
            .unwrap();

        let loaded = storage.load_label(label.id()).unwrap();
        assert!(loaded.contains_contact("alice-id"));

        storage
            .remove_contact_from_label(label.id(), "alice-id")
            .unwrap();

        let loaded = storage.load_label(label.id()).unwrap();
        assert!(!loaded.contains_contact("alice-id"));
    }

    #[test]
    fn test_remove_contact_from_all_labels() {
        let storage = test_storage();

        let label1 = storage.create_label("Label1").unwrap();
        let label2 = storage.create_label("Label2").unwrap();

        storage
            .add_contact_to_label(label1.id(), "alice-id")
            .unwrap();
        storage
            .add_contact_to_label(label2.id(), "alice-id")
            .unwrap();
        storage
            .save_contact_override("alice-id", "phone", true)
            .unwrap();

        storage.remove_contact_from_all_labels("alice-id").unwrap();

        let loaded1 = storage.load_label(label1.id()).unwrap();
        let loaded2 = storage.load_label(label2.id()).unwrap();
        let overrides = storage.load_contact_overrides("alice-id").unwrap();

        assert!(!loaded1.contains_contact("alice-id"));
        assert!(!loaded2.contains_contact("alice-id"));
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_set_label_field_visibility() {
        let storage = test_storage();

        let label = storage.create_label("Test").unwrap();

        storage
            .set_label_field_visibility(label.id(), "phone", true)
            .unwrap();
        storage
            .set_label_field_visibility(label.id(), "address", true)
            .unwrap();

        let loaded = storage.load_label(label.id()).unwrap();
        assert!(loaded.is_field_visible("phone"));
        assert!(loaded.is_field_visible("address"));

        storage
            .set_label_field_visibility(label.id(), "phone", false)
            .unwrap();

        let loaded = storage.load_label(label.id()).unwrap();
        assert!(!loaded.is_field_visible("phone"));
        assert!(loaded.is_field_visible("address"));
    }

    #[test]
    fn test_save_and_load_label_with_display_name_override() {
        let storage = test_storage();

        let mut label = VisibilityLabel::new("Professional");
        label.add_contact("alice-id");
        label.add_visible_field("work-email");
        label.set_display_name_override(Some("Dr. Egloff")).unwrap();

        storage.save_label(&label).unwrap();

        let loaded = storage.load_label(label.id()).unwrap();

        assert_eq!(loaded.name(), "Professional");
        assert!(loaded.contains_contact("alice-id"));
        assert!(loaded.is_field_visible("work-email"));
        assert_eq!(loaded.display_name_override(), Some("Dr. Egloff"));
    }

    #[test]
    fn test_save_and_load_label_without_display_name_override() {
        let storage = test_storage();

        let label = VisibilityLabel::new("Friends");
        storage.save_label(&label).unwrap();

        let loaded = storage.load_label(label.id()).unwrap();
        assert_eq!(loaded.display_name_override(), None);
    }

    #[test]
    fn test_load_all_labels_preserves_display_name_override() {
        let storage = test_storage();

        let mut label1 = VisibilityLabel::new("Family");
        label1.set_display_name_override(Some("Matt")).unwrap();

        let label2 = VisibilityLabel::new("Work");
        // label2 has no override

        storage.save_label(&label1).unwrap();
        storage.save_label(&label2).unwrap();

        let labels = storage.load_all_labels().unwrap();
        assert_eq!(labels.len(), 2);

        let family = labels.iter().find(|l| l.name() == "Family").unwrap();
        let work = labels.iter().find(|l| l.name() == "Work").unwrap();

        assert_eq!(family.display_name_override(), Some("Matt"));
        assert_eq!(work.display_name_override(), None);
    }

    #[test]
    fn test_display_name_override_roundtrip_update() {
        let storage = test_storage();

        // Create label with override
        let mut label = VisibilityLabel::new("Colleagues");
        label.set_display_name_override(Some("Dr. Egloff")).unwrap();
        storage.save_label(&label).unwrap();

        // Verify override persists
        let loaded = storage.load_label(label.id()).unwrap();
        assert_eq!(loaded.display_name_override(), Some("Dr. Egloff"));

        // Clear override and re-save
        let mut updated = loaded;
        updated.set_display_name_override(None).unwrap();
        storage.save_label(&updated).unwrap();

        // Verify override is cleared
        let reloaded = storage.load_label(label.id()).unwrap();
        assert_eq!(reloaded.display_name_override(), None);
    }

    #[test]
    fn test_get_labels_for_contact() {
        let storage = test_storage();

        let label1 = storage.create_label("Family").unwrap();
        let label2 = storage.create_label("Friends").unwrap();
        let _label3 = storage.create_label("Work").unwrap();

        storage
            .add_contact_to_label(label1.id(), "alice-id")
            .unwrap();
        storage
            .add_contact_to_label(label2.id(), "alice-id")
            .unwrap();

        let alice_labels = storage.get_labels_for_contact("alice-id").unwrap();
        assert_eq!(alice_labels.len(), 2);

        let names: Vec<_> = alice_labels.iter().map(|l| l.name()).collect();
        assert!(names.contains(&"Family"));
        assert!(names.contains(&"Friends"));
    }
}
