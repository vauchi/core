// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use rusqlite::Connection;

use super::StorageError;

pub(crate) struct MigrationDurability<'a> {
    conn: &'a Connection,
    previous: i64,
}

impl<'a> MigrationDurability<'a> {
    pub(crate) fn full(conn: &'a Connection) -> Result<Self, StorageError> {
        let previous: i64 = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
        conn.pragma_update(None, "synchronous", previous.max(2))?;
        Ok(Self { conn, previous })
    }
}

impl Drop for MigrationDurability<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.conn.pragma_update(None, "synchronous", self.previous) {
            // #1178: restoration cannot turn an already committed migration into failed open.
            tracing::warn!(target: "vauchi.storage.migration",
                sqlite_error_code = ?error.sqlite_error().map(|e| e.extended_code),
                "could not restore storage sync policy after migration");
        }
    }
}
