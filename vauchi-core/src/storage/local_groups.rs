// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage CRUD for local organization groups.
//!
//! Local groups are stored in the `local_groups` table (migration v35).
//! The group name is stored as plain text — it is a user-visible label
//! that has no security significance (unlike visibility label names).
//! `contact_ids_json` is an unencrypted JSON array of contact UUID strings.
//!
//! Groups have NO visibility/sharing semantics and are never transmitted.

use std::collections::HashSet;

use crate::contact::LocalGroup;

use super::{Storage, StorageError};

impl Storage {
    /// Creates and persists a new local group with the given name.
    pub fn create_local_group(&self, name: &str) -> Result<LocalGroup, StorageError> {
        let group = LocalGroup::new(name, self.clock().unix_seconds());
        let contact_ids_json = serde_json::to_string(&Vec::<String>::new())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.conn
            .execute(
                "INSERT INTO local_groups (id, name, contact_ids_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![group.id, group.name, contact_ids_json, group.created_at],
            )
            .map_err(StorageError::from)?;

        Ok(group)
    }

    /// Returns the local group with the given ID, or `None` if not found.
    pub fn get_local_group(&self, id: &str) -> Result<Option<LocalGroup>, StorageError> {
        let result = self.conn.query_row(
            "SELECT id, name, contact_ids_json, created_at FROM local_groups WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            },
        );

        match result {
            Ok((id, name, contact_ids_json, created_at)) => {
                let ids: Vec<String> = serde_json::from_str(&contact_ids_json)
                    .map_err(|e| StorageError::InvalidData(e.to_string()))?;
                Ok(Some(LocalGroup {
                    id,
                    name,
                    contact_ids: ids.into_iter().collect(),
                    created_at,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::from(e)),
        }
    }

    /// Lists all local groups, ordered by created_at ascending.
    pub fn list_local_groups(&self) -> Result<Vec<LocalGroup>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, contact_ids_json, created_at
                 FROM local_groups
                 ORDER BY created_at ASC",
            )
            .map_err(StorageError::from)?;

        let rows: Vec<(String, String, String, u64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })
            .map_err(StorageError::from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;

        rows.into_iter()
            .map(|(id, name, contact_ids_json, created_at)| {
                let ids: Vec<String> = serde_json::from_str(&contact_ids_json)
                    .map_err(|e| StorageError::InvalidData(e.to_string()))?;
                Ok(LocalGroup {
                    id,
                    name,
                    contact_ids: ids.into_iter().collect(),
                    created_at,
                })
            })
            .collect()
    }

    /// Deletes a local group by ID. Returns `true` if the group existed.
    pub fn delete_local_group(&self, id: &str) -> Result<bool, StorageError> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM local_groups WHERE id = ?1",
                rusqlite::params![id],
            )
            .map_err(StorageError::from)?;
        Ok(rows > 0)
    }

    /// Adds a contact to a local group.
    ///
    /// If the contact is already in the group this is a no-op (idempotent).
    /// Returns `Err(NotFound)` if the group does not exist.
    pub fn add_to_local_group(&self, group_id: &str, contact_id: &str) -> Result<(), StorageError> {
        self.update_local_group_membership(group_id, |ids| {
            ids.insert(contact_id.to_string());
        })
    }

    /// Removes a contact from a local group.
    ///
    /// If the contact is not in the group this is a no-op (idempotent).
    /// Returns `Err(NotFound)` if the group does not exist.
    pub fn remove_from_local_group(
        &self,
        group_id: &str,
        contact_id: &str,
    ) -> Result<(), StorageError> {
        self.update_local_group_membership(group_id, |ids| {
            ids.remove(contact_id);
        })
    }

    /// Helper: loads, modifies, and persists the contact_ids_json for a group.
    fn update_local_group_membership(
        &self,
        group_id: &str,
        modify: impl FnOnce(&mut HashSet<String>),
    ) -> Result<(), StorageError> {
        let current_json: String = self
            .conn
            .query_row(
                "SELECT contact_ids_json FROM local_groups WHERE id = ?1",
                rusqlite::params![group_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound("Local group not found".to_string())
                }
                other => StorageError::from(other),
            })?;

        let mut ids: HashSet<String> = {
            let list: Vec<String> = serde_json::from_str(&current_json)
                .map_err(|e| StorageError::InvalidData(e.to_string()))?;
            list.into_iter().collect()
        };

        modify(&mut ids);

        // Sort for deterministic storage
        let mut ids_sorted: Vec<String> = ids.into_iter().collect();
        ids_sorted.sort();
        let updated_json = serde_json::to_string(&ids_sorted)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.conn
            .execute(
                "UPDATE local_groups SET contact_ids_json = ?1 WHERE id = ?2",
                rusqlite::params![updated_json, group_id],
            )
            .map_err(StorageError::from)?;

        Ok(())
    }
}
