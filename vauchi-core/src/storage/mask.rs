// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one row of the real file the mask profile reaches (ADR-080,
//! vauchi/private#1117): a [`SealedSlot`] can read and write the
//! `duress_content` row and no other.

use rusqlite::{Connection, OptionalExtension};

use super::StorageError;

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
