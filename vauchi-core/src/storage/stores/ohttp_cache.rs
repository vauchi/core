// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OhttpCache domain persistence view (ohttp_key_cache).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
use rusqlite::{Connection, params};
use std::sync::Arc;

use super::super::{Storage, StorageError};

/// Scoped persistence view for the ohttp_cache domain.
pub struct OhttpCacheStore<'a> {
    conn: &'a Connection,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the ohttp_cache domain.
    pub fn ohttp_cache(&self) -> OhttpCacheStore<'_> {
        OhttpCacheStore {
            conn: &self.conn,
            clock: &self.clock,
        }
    }
}

impl OhttpCacheStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
    /// Save or replace the cached OHTTP key for a relay URL.
    ///
    /// Records the current Unix-epoch time as `fetched_at`. If a cached
    /// key already exists for this relay, it is overwritten (upsert).
    pub fn save_ohttp_key(&self, relay_url: &str, key_bytes: &[u8]) -> Result<(), StorageError> {
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO ohttp_key_cache (relay_url, key_bytes, fetched_at) \
             VALUES (?1, ?2, ?3)",
            params![relay_url, key_bytes, now as i64],
        )?;
        Ok(())
    }
    /// Load the cached OHTTP key for a relay URL.
    ///
    /// Returns `Ok(Some((key_bytes, fetched_at)))` if a cached key exists,
    /// or `Ok(None)` if the cache has no entry for this relay.
    pub fn load_ohttp_key(&self, relay_url: &str) -> Result<Option<(Vec<u8>, u64)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT key_bytes, fetched_at FROM ohttp_key_cache WHERE relay_url = ?1",
            params![relay_url],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        );
        match result {
            Ok((bytes, fetched_at)) => Ok(Some((bytes, fetched_at as u64))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Remove the cached OHTTP key for a relay URL.
    ///
    /// No-op if no entry exists for this relay.
    pub fn clear_ohttp_key(&self, relay_url: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM ohttp_key_cache WHERE relay_url = ?1",
            params![relay_url],
        )?;
        Ok(())
    }
}
