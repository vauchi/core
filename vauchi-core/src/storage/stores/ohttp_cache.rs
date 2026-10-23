// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OhttpCache domain persistence view (ohttp_key_cache).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
#[cfg(feature = "network-rustls")]
use crate::crypto::ohttp_key_trust::{HeldOhttpKey, OhttpAnchor};
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
    /// Keep `window` as the delay window `relay_url` advertises (#628).
    pub fn save_delay_window(
        &self,
        relay_url: &str,
        window: std::time::Duration,
    ) -> Result<(), StorageError> {
        let seconds = i64::try_from(window.as_secs())
            .map_err(|_| StorageError::Serialization("delay window out of range".into()))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO ohttp_delay_window (relay_url, seconds) VALUES (?1, ?2)",
            params![relay_url, seconds],
        )?;
        Ok(())
    }

    /// The delay window `relay_url` last advertised, if it still does.
    pub fn load_delay_window(
        &self,
        relay_url: &str,
    ) -> Result<Option<std::time::Duration>, StorageError> {
        let result = self.conn.query_row(
            "SELECT seconds FROM ohttp_delay_window WHERE relay_url = ?1",
            params![relay_url],
            |row| row.get::<_, i64>(0),
        );
        match result {
            Ok(seconds) => Ok(u64::try_from(seconds)
                .ok()
                .map(std::time::Duration::from_secs)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Forget `relay_url`'s delay window: it stopped advertising one.
    pub fn clear_delay_window(&self, relay_url: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM ohttp_delay_window WHERE relay_url = ?1",
            params![relay_url],
        )?;
        Ok(())
    }

    /// Note that `relay_url` stopped offering the delayed path now.
    pub fn record_delay_withdrawal(&self, relay_url: &str) -> Result<(), StorageError> {
        let now = self.now_secs();
        self.conn.execute(
            "INSERT OR REPLACE INTO ohttp_delay_withdrawn (relay_url, withdrawn_at) \
             VALUES (?1, ?2)",
            params![relay_url, now as i64],
        )?;
        Ok(())
    }

    /// When `relay_url` stopped offering the delayed path, unless it offers
    /// it again.
    pub fn load_delay_withdrawal(&self, relay_url: &str) -> Result<Option<u64>, StorageError> {
        let result = self.conn.query_row(
            "SELECT withdrawn_at FROM ohttp_delay_withdrawn WHERE relay_url = ?1",
            params![relay_url],
            |row| row.get::<_, i64>(0),
        );
        match result {
            Ok(at) => Ok(u64::try_from(at).ok()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Forget a withdrawal: `relay_url` offers the delayed path again.
    pub fn clear_delay_withdrawal(&self, relay_url: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM ohttp_delay_withdrawn WHERE relay_url = ?1",
            params![relay_url],
        )?;
        Ok(())
    }

    /// Keep `held` as the signed key for `relay_url` (#288), replacing the
    /// one held before.
    #[cfg(feature = "network-rustls")]
    pub fn save_held_ohttp_key(
        &self,
        relay_url: &str,
        held: &HeldOhttpKey,
    ) -> Result<(), StorageError> {
        let window = i64::try_from(held.window)
            .map_err(|_| StorageError::Serialization("OHTTP key window out of range".into()))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO ohttp_held_key (relay_url, window, key_config) \
             VALUES (?1, ?2, ?3)",
            params![relay_url, window, held.key_config],
        )?;
        Ok(())
    }

    /// The signed key held for `relay_url`, if any.
    #[cfg(feature = "network-rustls")]
    pub fn load_held_ohttp_key(
        &self,
        relay_url: &str,
    ) -> Result<Option<HeldOhttpKey>, StorageError> {
        let result = self.conn.query_row(
            "SELECT window, key_config FROM ohttp_held_key WHERE relay_url = ?1",
            params![relay_url],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        );
        match result {
            Ok((window, key_config)) => Ok(Some(HeldOhttpKey {
                window: u64::try_from(window).map_err(|_| {
                    StorageError::Serialization("OHTTP key window out of range".into())
                })?,
                key_config,
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Keep `reached`, the anchor followed from `configured` through
    /// `relay_url`'s rollover chain (#288 decision 0.13).
    #[cfg(feature = "network-rustls")]
    pub fn save_followed_anchor(
        &self,
        relay_url: &str,
        configured: &[u8; 32],
        reached: &OhttpAnchor,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ohttp_followed_anchor \
             (relay_url, configured_anchor, anchor, backup_commitment) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                relay_url,
                configured.as_slice(),
                reached.anchor.as_slice(),
                reached.backup_commitment.as_slice()
            ],
        )?;
        Ok(())
    }

    /// The anchor followed from `configured` for `relay_url`, if any; one
    /// reached from another configured anchor does not count.
    #[cfg(feature = "network-rustls")]
    pub fn load_followed_anchor(
        &self,
        relay_url: &str,
        configured: &[u8; 32],
    ) -> Result<Option<OhttpAnchor>, StorageError> {
        let result = self.conn.query_row(
            "SELECT anchor, backup_commitment FROM ohttp_followed_anchor \
             WHERE relay_url = ?1 AND configured_anchor = ?2",
            params![relay_url, configured.as_slice()],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        );
        let (anchor, backup_commitment) = match result {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(StorageError::Database(e)),
        };
        let malformed = || StorageError::Serialization("followed OHTTP anchor malformed".into());
        Ok(Some(OhttpAnchor {
            anchor: anchor.try_into().map_err(|_| malformed())?,
            backup_commitment: backup_commitment.try_into().map_err(|_| malformed())?,
        }))
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
