// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! PinCache domain persistence view (pin_cache).
//!
//! Part of problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
use rusqlite::{Connection, params};
use std::sync::Arc;

use super::super::{Storage, StorageError};
use crate::network::PinnedCertificate;

/// Scoped persistence view for the pin_cache domain.
pub struct PinCacheStore<'a> {
    conn: &'a Connection,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the pin_cache domain.
    pub fn pin_cache(&self) -> PinCacheStore<'_> {
        PinCacheStore {
            conn: &self.conn,
            clock: &self.clock,
        }
    }
}

impl PinCacheStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
    /// Save or replace cached certificate pins for a relay URL.
    ///
    /// Serializes pins as concatenated 32-byte SHA-256 fingerprints.
    /// Records the current Unix-epoch time as `fetched_at`.
    pub fn save_pin_cache(
        &self,
        relay_url: &str,
        pins: &[PinnedCertificate],
    ) -> Result<(), StorageError> {
        let now = self.now_secs();
        let pin_bytes: Vec<u8> = pins.iter().flat_map(|p| p.sha256_fingerprint).collect();
        self.conn.execute(
            "INSERT OR REPLACE INTO pin_cache (relay_url, pin_bytes, fetched_at) \
             VALUES (?1, ?2, ?3)",
            params![relay_url, pin_bytes, now as i64],
        )?;
        Ok(())
    }
    /// Load cached certificate pins for a relay URL.
    ///
    /// Returns `Ok(Some((pins, fetched_at)))` if cached pins exist,
    /// or `Ok(None)` if no entry exists for this relay.
    pub fn load_pin_cache(
        &self,
        relay_url: &str,
    ) -> Result<Option<(Vec<PinnedCertificate>, u64)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT pin_bytes, fetched_at FROM pin_cache WHERE relay_url = ?1",
            params![relay_url],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        );
        match result {
            Ok((bytes, fetched_at)) => {
                let pins = bytes
                    .chunks_exact(32)
                    .map(|chunk| {
                        let mut fingerprint = [0u8; 32];
                        fingerprint.copy_from_slice(chunk);
                        PinnedCertificate::new(fingerprint)
                    })
                    .collect();
                Ok(Some((pins, fetched_at as u64)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Remove cached certificate pins for a relay URL.
    ///
    /// No-op if no entry exists for this relay.
    pub fn clear_pin_cache(&self, relay_url: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM pin_cache WHERE relay_url = ?1",
            params![relay_url],
        )?;
        Ok(())
    }
}
