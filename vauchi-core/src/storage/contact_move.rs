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
use crate::crypto::SymmetricKey;

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

/// The owner's own annotations on a contact, which a fold fills in on the
/// new contact where it has none.
const FILLED_ON_FOLD: &[&str] = &[
    "personal_notes_encrypted",
    "nickname_encrypted",
    "custom_avatar_encrypted",
];

/// The owner's marks on a contact, which a fold keeps when either contact
/// had them (a guardian stays one).
const KEPT_SET_ON_FOLD: &[&str] = &["favorite", "proposal_trusted", "recovery_trusted"];

/// Tables holding the old identity's devices and its device-registry
/// handshake, which end with the old relationship: the new identity has
/// other devices, and the contact runs the handshake with it again (D25).
const ENDED_WITH_THE_OLD_IDENTITY: &[&str] = &["contact_device_registries", "registry_activation"];

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
        self.in_both_databases(|| self.move_references(old_id, new_id, false))
    }

    /// Folds the contact `old_id` into the contact `new_id` (D21), when both
    /// exist: the new one keeps its key, shared key, ratchet, card and the
    /// choices made at its exchange; what only the old one held comes
    /// across (the owner's notes, nickname, custom avatar, field notes,
    /// labels, tags, guardian roles, kept backups, history); a row both
    /// hold keeps the new one's; then the old contact goes.
    ///
    /// # Errors
    /// [`StorageError::NotFound`] when either is not a contact; nothing
    /// changes then, nor on any other error.
    pub fn fold_contact_into(&self, old_id: &str, new_id: &str) -> Result<(), StorageError> {
        for id in [old_id, new_id] {
            if self.contacts().load_contact(id)?.is_none() {
                return Err(StorageError::NotFound(id.to_string()));
            }
        }
        self.in_both_databases(|| self.move_references(old_id, new_id, true))
    }

    /// Runs `operation` in a savepoint on both databases and seals the
    /// duress content, or rolls both back.
    fn in_both_databases(
        &self,
        operation: impl FnOnce() -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        let begin = format!("SAVEPOINT {MOVE_SAVEPOINT}");
        self.mask.conn().execute_batch(&begin)?;
        if let Err(error) = self.conn.execute_batch(&begin) {
            rollback(self.mask.conn());
            return Err(error.into());
        }
        let moved =
            operation().and_then(|()| self.mask.seal_into(super::mask::SealedSlot::of(&self.conn)));
        match moved {
            Ok(()) => {
                let release = format!("RELEASE {MOVE_SAVEPOINT}");
                self.conn.execute_batch(&release)?;
                self.mask.conn().execute_batch(&release)?;
                Ok(())
            }
            Err(error) => {
                rollback(&self.conn);
                rollback(self.mask.conn());
                Err(error)
            }
        }
    }

    /// Points everything naming `old_id` at `new_id`. A move renames the
    /// contacts row; a fold keeps the new row, takes the owner's
    /// annotations from the old one, keeps the new contact's row wherever
    /// both have one, and removes the old contact and what is left of it.
    fn move_references(&self, old_id: &str, new_id: &str, fold: bool) -> Result<(), StorageError> {
        // The children point at the new id only after the parent row moved;
        // the foreign keys are checked when the transaction commits.
        self.conn.execute_batch("PRAGMA defer_foreign_keys = ON")?;
        let update = if fold { "UPDATE OR IGNORE" } else { "UPDATE" };
        for table in ENDED_WITH_THE_OLD_IDENTITY {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE contact_id = ?1"),
                params![old_id],
            )?;
        }
        if fold {
            self.fold_annotations(old_id, new_id)?;
            // The old relationship's ratchets end with it; the new contact
            // has the meeting's.
            self.conn.execute(
                "DELETE FROM contact_ratchets WHERE contact_id = ?1",
                params![old_id],
            )?;
        } else {
            self.conn.execute(
                "UPDATE contacts SET id = ?2 WHERE id = ?1",
                params![old_id, new_id],
            )?;
        }
        for (table, column) in foreign_keys_to_contacts(&self.conn)? {
            self.conn.execute(
                &format!("{update} {table} SET {column} = ?2 WHERE {column} = ?1"),
                params![old_id, new_id],
            )?;
        }
        for (table, column, reference) in CONTACT_REFERENCES {
            let conn = self.table_conn_for_move(table);
            match reference {
                ContactReference::Id => {
                    conn.execute(
                        &format!("{update} {table} SET {column} = ?2 WHERE {column} = ?1"),
                        params![old_id, new_id],
                    )?;
                    if fold {
                        conn.execute(
                            &format!("DELETE FROM {table} WHERE {column} = ?1"),
                            params![old_id],
                        )?;
                    }
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
                &format!("{update} {table} SET designator_tag = ?2 WHERE designator_tag = ?1"),
                params![old_tag, new_tag],
            )?;
            if fold {
                self.conn.execute(
                    &format!("DELETE FROM {table} WHERE designator_tag = ?1"),
                    params![old_tag],
                )?;
            }
        }
        if fold {
            // What the foreign keys still tie to the old contact is what
            // the new one already had; it goes with the old contact.
            self.conn
                .execute("DELETE FROM contacts WHERE id = ?1", params![old_id])?;
        }
        Ok(())
    }

    /// The owner's annotations on the old contact come to the new one: a
    /// value the new contact lacks is filled, and a mark set on either
    /// stays set.
    fn fold_annotations(&self, old_id: &str, new_id: &str) -> Result<(), StorageError> {
        for column in FILLED_ON_FOLD {
            self.conn.execute(
                &format!(
                    "UPDATE contacts SET {column} = (SELECT {column} FROM contacts WHERE id = ?1)
                     WHERE id = ?2 AND {column} IS NULL"
                ),
                params![old_id, new_id],
            )?;
        }
        for column in KEPT_SET_ON_FOLD {
            self.conn.execute(
                &format!(
                    "UPDATE contacts SET {column} = MAX({column}, (SELECT {column} FROM contacts WHERE id = ?1))
                     WHERE id = ?2"
                ),
                params![old_id, new_id],
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
                crate::crypto::decrypt(self.table_key_for_move(table), &stored)
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
                let sealed = crate::crypto::encrypt(self.table_key_for_move(table), &json)
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

    fn table_key_for_move(&self, table: &str) -> &SymmetricKey {
        if DURESS_TABLES.contains(&table) {
            self.mask.key()
        } else {
            &self.encryption_key
        }
    }

    fn table_conn_for_move(&self, table: &str) -> &Connection {
        if DURESS_TABLES.contains(&table) {
            self.mask.conn()
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
