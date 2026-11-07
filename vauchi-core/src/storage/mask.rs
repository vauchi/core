// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the mask profile holds (ADR-080, vauchi/private#1117): the duress
//! database and the mask key, nothing of the real profile. Its seal reaches
//! the database file only through a [`SealedSlot`], which can read and
//! write the one `duress_content` row and no other.

use rusqlite::{Connection, OptionalExtension};

use crate::crypto::SymmetricKey;

use super::StorageError;
use super::duress_content::{mask_key, open_duress_db, seal};

pub(crate) struct MaskStorage {
    conn: Connection,
    key: SymmetricKey,
}

impl MaskStorage {
    /// An empty duress database under the mask key of `storage_key`.
    pub(super) fn new(storage_key: &SymmetricKey) -> Result<Self, StorageError> {
        Ok(MaskStorage {
            conn: open_duress_db()?,
            key: mask_key(storage_key),
        })
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn key(&self) -> &SymmetricKey {
        &self.key
    }

    /// After a storage rekey has moved the fields and the seal to `key`.
    pub(super) fn replace_key(&mut self, key: SymmetricKey) {
        self.key = key;
    }

    /// Seals the duress database into `slot`.
    pub(crate) fn seal_into(&self, slot: SealedSlot<'_>) -> Result<(), StorageError> {
        slot.write(&seal(&self.key, &self.conn)?)
    }
}

/// The file's `duress_content` row, the only part of the real database the
/// mask's content is written to.
#[derive(Clone, Copy)]
pub(crate) struct SealedSlot<'a> {
    file: &'a Connection,
}

impl<'a> SealedSlot<'a> {
    pub(crate) fn of(file: &'a Connection) -> Self {
        SealedSlot { file }
    }

    pub(crate) fn read(self) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self
            .file
            .query_row(
                "SELECT content_encrypted FROM duress_content WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(crate) fn write(self, sealed: &[u8]) -> Result<(), StorageError> {
        self.file.execute(
            "INSERT INTO duress_content (id, content_encrypted) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET content_encrypted = excluded.content_encrypted",
            [sealed],
        )?;
        Ok(())
    }
}
