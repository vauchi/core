// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one row of the real file the mask profile reaches (ADR-080,
//! vauchi/private#1117): a [`SealedSlot`] can read and write the
//! `duress_content` row and no other.

use rusqlite::{Connection, OptionalExtension, params};

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

    pub(crate) fn read_observed(self) -> Result<Option<Vec<u8>>, StorageError> {
        let sealed = self.read()?;
        self.remember(sealed.as_deref())?;
        Ok(sealed)
    }

    fn ensure_observation(self) -> Result<(), StorageError> {
        self.file.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS mask_seal_seen (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                content_encrypted BLOB
            )",
        )?;
        Ok(())
    }

    pub(crate) fn observed(self) -> Result<Option<Vec<u8>>, StorageError> {
        self.ensure_observation()?;
        Ok(self
            .file
            .query_row(
                "SELECT content_encrypted FROM temp.mask_seal_seen WHERE id = 1",
                [],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten())
    }

    pub(crate) fn remember(self, sealed: Option<&[u8]>) -> Result<(), StorageError> {
        self.ensure_observation()?;
        self.file.execute(
            "INSERT INTO temp.mask_seal_seen (id, content_encrypted) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET content_encrypted = excluded.content_encrypted",
            [sealed],
        )?;
        Ok(())
    }

    /// A memory profile may replace only the file image it actually loaded.
    /// The observation lives on this connection, so an outer rollback also
    /// rolls it back (ADR-080; notice-core section 7, private#1375).
    pub(crate) fn write_observed(self, sealed: &[u8]) -> Result<(), StorageError> {
        let previous = self.observed()?;
        let changed = self.file.execute(
            "INSERT INTO duress_content (id, content_encrypted)
             SELECT 1, ?1 WHERE
                 (?2 IS NULL AND NOT EXISTS (SELECT 1 FROM duress_content WHERE id = 1))
                 OR EXISTS (SELECT 1 FROM duress_content WHERE id = 1 AND content_encrypted = ?2)
             ON CONFLICT(id) DO UPDATE SET content_encrypted = excluded.content_encrypted
             WHERE duress_content.content_encrypted = ?2",
            params![sealed, previous],
        )?;
        if changed != 1 {
            return Err(StorageError::NoticeMoved);
        }
        self.remember(Some(sealed))
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
