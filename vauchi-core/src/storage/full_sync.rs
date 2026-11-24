// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use rusqlite::Connection;

use super::StorageError;

/// Raises the connection to at least FULL synchronous durability while it
/// lives, for writes a power loss must not undo, and restores the previous
/// policy when dropped.
pub(crate) struct FullSync<'a> {
    conn: &'a Connection,
    previous: i64,
}

impl<'a> FullSync<'a> {
    pub(crate) fn full(conn: &'a Connection) -> Result<Self, StorageError> {
        let previous: i64 = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
        conn.pragma_update(None, "synchronous", previous.max(2))?;
        Ok(Self { conn, previous })
    }
}

impl Drop for FullSync<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.conn.pragma_update(None, "synchronous", self.previous) {
            // #1178: restoration cannot turn an already committed write into a failure.
            tracing::warn!(target: "vauchi.storage",
                sqlite_error_code = ?error.sqlite_error().map(|e| e.extended_code),
                "could not restore storage sync policy after a durable write");
        }
    }
}
