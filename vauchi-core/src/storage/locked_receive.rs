// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The database as a locked run opens it: with the Background Receive Key
//! alone (ADR-033 amendment 2026-11-07, vauchi/private#1093 slice 6). The
//! type holds no storage key, so nothing sealed under it can be read or
//! written here; the receive state under the BRK and the inbox's write side
//! are all it offers.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, OpenFlags};

use crate::clock::Clock;
use crate::crypto::SymmetricKey;

use super::migration::{MigrationRunner, all_migrations};
use super::receive_key::{ReceiveKeyCheck, check_receive_key};
use super::{
    KeptBlobStore, RatchetStore, ReceiveInboxStore, ReceiveTokenStore, Storage, StorageError,
};

/// The database opened with the Background Receive Key alone.
pub struct LockedReceiveStorage {
    conn: Connection,
    receive_key: SymmetricKey,
    clock: Arc<dyn Clock>,
}

impl LockedReceiveStorage {
    /// Opens the file at `path` with `receive_key`.
    ///
    /// The file is never created or migrated here: a missing file is
    /// [`StorageError::NotFound`], and one an older build left unmigrated is
    /// [`StorageError::StorageKeyUnavailable`] until the unlock migrates it.
    /// A file never given a receive key is
    /// [`StorageError::ReceiveKeyUnavailable`]; another key is
    /// [`StorageError::WrongKey`].
    pub fn open<P: AsRef<Path>>(path: P, receive_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = match Connection::open_with_flags(
            path.as_ref(),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(conn) => conn,
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::CannotOpen =>
            {
                return Err(StorageError::NotFound("database".into()));
            }
            Err(e) => return Err(e.into()),
        };
        Storage::configure_pragmas(&conn)?;
        let newest = all_migrations().last().map_or(0, |m| m.version);
        if MigrationRunner::current_version(&conn)? < newest {
            return Err(StorageError::StorageKeyUnavailable);
        }
        match check_receive_key(&conn, &receive_key)? {
            ReceiveKeyCheck::Opens => {}
            ReceiveKeyCheck::Absent => return Err(StorageError::ReceiveKeyUnavailable),
            ReceiveKeyCheck::Refused => return Err(StorageError::WrongKey),
        }
        Ok(Self {
            conn,
            receive_key,
            clock: crate::clock::SystemClock::shared(),
        })
    }

    /// Runs `writes` in one transaction: all of them commit, or none.
    pub fn in_transaction<T>(
        &self,
        writes: impl FnOnce(&Self) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        match writes(self) {
            Ok(value) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(e) => {
                // A failed rollback leaves SQLite to undo the open
                // transaction when the connection closes; the write error is
                // the one the caller needs.
                if let Err(rollback) = self.conn.execute_batch("ROLLBACK") {
                    tracing::warn!(
                        target: "vauchi.storage.locked_receive",
                        error = %rollback,
                        "rollback failed; the connection's close undoes it"
                    );
                }
                Err(e)
            }
        }
    }

    /// Replaces the clock the stores stamp rows with.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The ratchet rows under the receive key; one still under the storage
    /// key reads as [`StorageError::StorageKeyUnavailable`].
    pub fn ratchets(&self) -> RatchetStore<'_> {
        RatchetStore::under_receive_key(&self.conn, &self.receive_key, &self.clock)
    }

    /// The OHTTP key and delay-window cache, which no key seals.
    pub fn ohttp_cache(&self) -> super::OhttpCacheStore<'_> {
        super::OhttpCacheStore::unsealed(&self.conn, &self.clock)
    }

    /// The blobs a locked run kept, to acknowledge without trying again.
    pub fn kept_blobs(&self) -> KeptBlobStore<'_> {
        KeptBlobStore::new(&self.conn, Some(&self.receive_key))
    }

    /// The receive registrations stored ahead.
    pub fn receive_tokens(&self) -> ReceiveTokenStore<'_> {
        ReceiveTokenStore::under_receive_key(&self.conn, &self.receive_key)
    }

    /// The inbox, to put items in; reading it needs the storage key.
    pub fn receive_inbox(&self) -> ReceiveInboxStore<'_> {
        ReceiveInboxStore::write_only(&self.conn, &self.receive_key)
    }
}
