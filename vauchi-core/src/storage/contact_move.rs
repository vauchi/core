// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Moving a contact to a new id (vauchi/private#796, step 5b).
//!
//! A contact's id is the hex of its identity key, so a contact that accepts
//! a recovery moves to the new key's id. Everything that names the contact
//! follows in one transaction: the tables with a foreign key to
//! `contacts(id)` are found from the schema, the other columns are listed
//! in [`CONTACT_REFERENCES`], and the guardian tables' keyed tag of the
//! designator is recomputed. A schema test fails on any column naming a
//! contact that is neither, so a new table cannot be missed.

use rusqlite::{Connection, params};

use super::duress_content::DURESS_TABLES;
use super::{Storage, StorageError};

/// How a move treats a column that names a contact without a foreign key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactReference {
    /// Holds the contact's id; it moves.
    Id,
    /// Holds state of the old relationship (its ratchet's messages and
    /// nonces), which the rekey ends; its rows go.
    StaleId,
    /// A JSON array of contact ids, in the clear.
    IdList,
    /// A JSON array of contact ids, encrypted with the storage key.
    EncryptedIdList,
    /// Names contacts but holds no contact id.
    NotAnId,
}

/// The columns naming a contact without a foreign key to `contacts(id)`.
pub const CONTACT_REFERENCES: &[(&str, &str, ContactReference)] = &[
    ("activity_log", "contact_id", ContactReference::Id),
    (
        "contact_device_delta_versions",
        "contact_id",
        ContactReference::Id,
    ),
    (
        "contact_sync_timestamps",
        "contact_id",
        ContactReference::Id,
    ),
    (
        "contact_visibility_overrides",
        "contact_id",
        ContactReference::Id,
    ),
    (
        "duress_contact_visibility_overrides",
        "contact_id",
        ContactReference::Id,
    ),
    ("field_validations", "contact_id", ContactReference::Id),
    ("recovery_responses", "contact_id", ContactReference::Id),
    ("resync_honored", "contact_id", ContactReference::Id),
    ("pending_updates", "contact_id", ContactReference::StaleId),
    ("replay_nonces", "contact_id", ContactReference::StaleId),
    ("tags", "contact_ids_json", ContactReference::IdList),
    ("duress_tags", "contact_ids_json", ContactReference::IdList),
    ("local_groups", "contact_ids_json", ContactReference::IdList),
    (
        "visibility_labels",
        "contacts_json_encrypted",
        ContactReference::EncryptedIdList,
    ),
    (
        "duress_visibility_labels",
        "contacts_json_encrypted",
        ContactReference::EncryptedIdList,
    ),
    (
        "emergency_config",
        "trusted_contact_ids_encrypted",
        ContactReference::EncryptedIdList,
    ),
    ("contacts", "id", ContactReference::NotAnId),
    ("contacts", "contact_kind", ContactReference::NotAnId),
    ("contact_limits", "max_contacts", ContactReference::NotAnId),
    (
        "ux_state",
        "demo_contact_json_encrypted",
        ContactReference::NotAnId,
    ),
];

const MOVE_SAVEPOINT: &str = "vauchi_contact_move";

impl Storage {
    /// Moves the contact `old_id` to `new_id`, with everything that names
    /// it, in one transaction over both databases.
    ///
    /// # Errors
    /// [`StorageError::NotFound`] when `old_id` is not a contact,
    /// [`StorageError::InvalidData`] when `new_id` already is one; nothing
    /// changes then, nor on any other error.
    pub fn move_contact_id(&self, old_id: &str, new_id: &str) -> Result<(), StorageError> {
        if self.contacts().load_contact(new_id)?.is_some() {
            return Err(StorageError::InvalidData(
                "a contact with the new id exists".into(),
            ));
        }
        if self.contacts().load_contact(old_id)?.is_none() {
            return Err(StorageError::NotFound(old_id.to_string()));
        }
        let begin = format!("SAVEPOINT {MOVE_SAVEPOINT}");
        self.duress_db.execute_batch(&begin)?;
        if let Err(error) = self.conn.execute_batch(&begin) {
            rollback(&self.duress_db);
            return Err(error.into());
        }
        let moved = self.move_references(old_id, new_id).and_then(|()| {
            super::duress_content::write_sealed(&self.conn, &self.duress_db, &self.encryption_key)
        });
        match moved {
            Ok(()) => {
                let release = format!("RELEASE {MOVE_SAVEPOINT}");
                self.conn.execute_batch(&release)?;
                self.duress_db.execute_batch(&release)?;
                Ok(())
            }
            Err(error) => {
                rollback(&self.conn);
                rollback(&self.duress_db);
                Err(error)
            }
        }
    }

    fn move_references(&self, old_id: &str, new_id: &str) -> Result<(), StorageError> {
        // The children point at the new id only after the parent row moved;
        // the foreign keys are checked when the transaction commits.
        self.conn.execute_batch("PRAGMA defer_foreign_keys = ON")?;
        self.conn.execute(
            "UPDATE contacts SET id = ?2 WHERE id = ?1",
            params![old_id, new_id],
        )?;
        for (table, column) in foreign_keys_to_contacts(&self.conn)? {
            self.conn.execute(
                &format!("UPDATE {table} SET {column} = ?2 WHERE {column} = ?1"),
                params![old_id, new_id],
            )?;
        }
        for (table, column, reference) in CONTACT_REFERENCES {
            let conn = self.table_conn_for_move(table);
            match reference {
                ContactReference::Id => {
                    conn.execute(
                        &format!("UPDATE {table} SET {column} = ?2 WHERE {column} = ?1"),
                        params![old_id, new_id],
                    )?;
                }
                ContactReference::StaleId => {
                    conn.execute(
                        &format!("DELETE FROM {table} WHERE {column} = ?1"),
                        params![old_id],
                    )?;
                }
                ContactReference::IdList => {
                    self.rewrite_id_lists(conn, table, column, false, old_id, new_id)?;
                }
                ContactReference::EncryptedIdList => {
                    self.rewrite_id_lists(conn, table, column, true, old_id, new_id)?;
                }
                ContactReference::NotAnId => {}
            }
        }
        let old_tag = super::stores::designator_tag(&self.encryption_key, old_id);
        let new_tag = super::stores::designator_tag(&self.encryption_key, new_id);
        for table in ["guardian_backups", "guardian_backup_pieces"] {
            self.conn.execute(
                &format!("UPDATE {table} SET designator_tag = ?2 WHERE designator_tag = ?1"),
                params![old_tag, new_tag],
            )?;
        }
        Ok(())
    }

    /// Replaces `old_id` with `new_id` in each row's JSON array of ids.
    fn rewrite_id_lists(
        &self,
        conn: &Connection,
        table: &str,
        column: &str,
        encrypted: bool,
        old_id: &str,
        new_id: &str,
    ) -> Result<(), StorageError> {
        let rows: Vec<(i64, Option<Vec<u8>>)> = conn
            .prepare(&format!(
                "SELECT rowid, CAST({column} AS BLOB) FROM {table}"
            ))?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (rowid, stored) in rows {
            let Some(stored) = stored else { continue };
            let json = if encrypted {
                crate::crypto::decrypt(&self.encryption_key, &stored)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?
            } else {
                stored
            };
            let mut ids: Vec<String> = serde_json::from_slice(&json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            if !ids.iter().any(|id| id == old_id) {
                continue;
            }
            ids.retain(|id| id != old_id && id != new_id);
            ids.push(new_id.to_string());
            let json =
                serde_json::to_vec(&ids).map_err(|e| StorageError::Serialization(e.to_string()))?;
            if encrypted {
                let sealed = crate::crypto::encrypt(&self.encryption_key, &json)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                conn.execute(
                    &format!("UPDATE {table} SET {column} = ?1 WHERE rowid = ?2"),
                    params![sealed, rowid],
                )?;
            } else {
                let text = String::from_utf8(json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                conn.execute(
                    &format!("UPDATE {table} SET {column} = ?1 WHERE rowid = ?2"),
                    params![text, rowid],
                )?;
            }
        }
        Ok(())
    }

    fn table_conn_for_move(&self, table: &str) -> &Connection {
        if DURESS_TABLES.contains(&table) {
            &self.duress_db
        } else {
            &self.conn
        }
    }
}

/// Every `(table, column)` with a foreign key to `contacts(id)`.
fn foreign_keys_to_contacts(conn: &Connection) -> Result<Vec<(String, String)>, StorageError> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let mut found = Vec::new();
    for table in tables {
        let keys: Vec<(String, String)> = conn
            .prepare(&format!("PRAGMA foreign_key_list({table})"))?
            .query_map([], |row| Ok((row.get(2)?, row.get(3)?)))?
            .collect::<Result<_, _>>()?;
        found.extend(
            keys.into_iter()
                .filter(|(parent, _)| parent == "contacts")
                .map(|(_, column)| (table.clone(), column)),
        );
    }
    Ok(found)
}

/// Best-effort: the move already failed, and a failed rollback leaves the
/// savepoint to the connection's next transaction end.
fn rollback(conn: &Connection) {
    let undo = format!("ROLLBACK TO {MOVE_SAVEPOINT}; RELEASE {MOVE_SAVEPOINT}");
    if let Err(_already_failing) = conn.execute_batch(&undo) {}
}
