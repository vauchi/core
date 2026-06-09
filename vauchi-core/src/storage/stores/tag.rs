// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tag domain persistence view.
//!
//! `TagStore` owns the `tags` table (migration v49). Tag names are
//! owner-private and encrypted at rest (`name_encrypted`, ADR-051);
//! `contact_ids_json` is plaintext. Part of problem record
//! `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use std::collections::HashSet;
use std::sync::Arc;

use rusqlite::Connection;

use super::super::{Storage, StorageError};
use crate::clock::Clock;
use crate::contact::Tag;
use crate::crypto::SymmetricKey;

/// Scoped persistence view for contact tags.
pub struct TagStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the tag domain.
    pub fn tags(&self) -> TagStore<'_> {
        TagStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl TagStore<'_> {
    /// Creates and persists a new tag with the given name.
    pub fn create_tag(&self, name: &str) -> Result<Tag, StorageError> {
        let tag = Tag::new(name, self.clock.unix_seconds());
        let name_encrypted = crate::crypto::encrypt(self.key, name.as_bytes())
            .map_err(|e| StorageError::Encryption(format!("Encrypt tag name: {e}")))?;
        let contact_ids_json = serde_json::to_string(&Vec::<String>::new())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.conn
            .execute(
                "INSERT INTO tags (id, name_encrypted, contact_ids_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![tag.id, name_encrypted, contact_ids_json, tag.created_at],
            )
            .map_err(StorageError::from)?;

        Ok(tag)
    }

    /// Inserts or replaces a tag with a caller-supplied id, preserving its
    /// `contact_ids` and `created_at`. Used by device sync to restore tags with
    /// their original ids (unlike [`TagStore::create_tag`], which generates a new
    /// id). The name is encrypted at rest.
    pub fn save_tag(&self, tag: &Tag) -> Result<(), StorageError> {
        let name_encrypted = crate::crypto::encrypt(self.key, tag.name.as_bytes())
            .map_err(|e| StorageError::Encryption(format!("Encrypt tag name: {e}")))?;
        let mut ids: Vec<String> = tag.contact_ids.iter().cloned().collect();
        ids.sort();
        let contact_ids_json =
            serde_json::to_string(&ids).map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO tags (id, name_encrypted, contact_ids_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![tag.id, name_encrypted, contact_ids_json, tag.created_at],
            )
            .map_err(StorageError::from)?;
        Ok(())
    }

    /// Returns the tag with the given ID, or `None` if not found.
    pub fn get_tag(&self, id: &str) -> Result<Option<Tag>, StorageError> {
        let result = self.conn.query_row(
            "SELECT id, name_encrypted, contact_ids_json, created_at FROM tags WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            },
        );

        match result {
            Ok(row) => Ok(Some(self.row_to_tag(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::from(e)),
        }
    }

    /// Lists all tags, ordered by created_at ascending.
    pub fn list_tags(&self) -> Result<Vec<Tag>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name_encrypted, contact_ids_json, created_at
                 FROM tags
                 ORDER BY created_at ASC",
            )
            .map_err(StorageError::from)?;

        let rows: Vec<(String, Vec<u8>, String, u64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })
            .map_err(StorageError::from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;

        rows.into_iter().map(|row| self.row_to_tag(row)).collect()
    }

    /// Deletes a tag by ID. Returns `true` if the tag existed.
    pub fn delete_tag(&self, id: &str) -> Result<bool, StorageError> {
        let rows = self
            .conn
            .execute("DELETE FROM tags WHERE id = ?1", rusqlite::params![id])
            .map_err(StorageError::from)?;
        Ok(rows > 0)
    }

    /// Applies a tag to a contact. Idempotent. `Err(NotFound)` if no such tag.
    pub fn add_to_tag(&self, tag_id: &str, contact_id: &str) -> Result<(), StorageError> {
        self.update_tag_membership(tag_id, |ids| {
            ids.insert(contact_id.to_string());
        })
    }

    /// Removes a tag from a contact. Idempotent. `Err(NotFound)` if no such tag.
    pub fn remove_from_tag(&self, tag_id: &str, contact_id: &str) -> Result<(), StorageError> {
        self.update_tag_membership(tag_id, |ids| {
            ids.remove(contact_id);
        })
    }

    /// Decrypts a raw `(id, name_encrypted, contact_ids_json, created_at)` row
    /// into a `Tag`.
    fn row_to_tag(
        &self,
        (id, name_encrypted, contact_ids_json, created_at): (String, Vec<u8>, String, u64),
    ) -> Result<Tag, StorageError> {
        let name_bytes = crate::crypto::decrypt(self.key, &name_encrypted)
            .map_err(|e| StorageError::Encryption(format!("Decrypt tag name: {e}")))?;
        let name = String::from_utf8(name_bytes)
            .map_err(|e| StorageError::InvalidData(format!("Tag name not UTF-8: {e}")))?;
        let ids: Vec<String> = serde_json::from_str(&contact_ids_json)
            .map_err(|e| StorageError::InvalidData(e.to_string()))?;
        Ok(Tag {
            id,
            name,
            contact_ids: ids.into_iter().collect(),
            created_at,
        })
    }

    /// Helper: loads, modifies, and persists `contact_ids_json` for a tag.
    fn update_tag_membership(
        &self,
        tag_id: &str,
        modify: impl FnOnce(&mut HashSet<String>),
    ) -> Result<(), StorageError> {
        let current_json: String = self
            .conn
            .query_row(
                "SELECT contact_ids_json FROM tags WHERE id = ?1",
                rusqlite::params![tag_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound("Tag not found".to_string())
                }
                other => StorageError::from(other),
            })?;

        let mut ids: HashSet<String> = {
            let list: Vec<String> = serde_json::from_str(&current_json)
                .map_err(|e| StorageError::InvalidData(e.to_string()))?;
            list.into_iter().collect()
        };

        modify(&mut ids);

        // Sort for deterministic storage.
        let mut ids_sorted: Vec<String> = ids.into_iter().collect();
        ids_sorted.sort();
        let updated_json = serde_json::to_string(&ids_sorted)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.conn
            .execute(
                "UPDATE tags SET contact_ids_json = ?1 WHERE id = ?2",
                rusqlite::params![updated_json, tag_id],
            )
            .map_err(StorageError::from)?;

        Ok(())
    }
}
