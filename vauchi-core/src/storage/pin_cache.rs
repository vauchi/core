// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Certificate pin cache storage methods.
//!
//! Stores the most recently fetched TLS certificate pins per relay URL,
//! enabling pin rotation without app updates. Mirrors the OHTTP key
//! cache pattern (see `ohttp_cache.rs`).

use rusqlite::params;

use crate::network::PinnedCertificate;

use super::{Storage, StorageError};

impl Storage {
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
