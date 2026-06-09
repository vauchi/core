// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Replay-nonce persistence view (ADR-029 replay defense).
//!
//! `ReplayStore` owns the `replay_nonces` table. Nonces are not encrypted (they
//! are random opaque values), so this store needs no encryption key. Part of
//! problem record `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use rusqlite::{Connection, params};

use super::super::{Storage, StorageError};

/// Scoped persistence view for replay-nonce defense.
pub struct ReplayStore<'a> {
    conn: &'a Connection,
}

impl Storage {
    /// Scoped persistence view for the replay-nonce domain.
    pub fn replay(&self) -> ReplayStore<'_> {
        ReplayStore { conn: &self.conn }
    }
}

impl ReplayStore<'_> {
    /// Saves a replay nonce for a contact.
    ///
    /// Uses INSERT OR IGNORE to be idempotent if the nonce already exists.
    pub fn save_replay_nonce(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO replay_nonces (contact_id, nonce, timestamp)
             VALUES (?1, ?2, ?3)",
            params![contact_id, nonce.as_slice(), timestamp as i64],
        )?;
        Ok(())
    }

    /// Test-only: insert a replay-nonce row with arbitrary (possibly
    /// malformed) BLOB length, used to exercise the
    /// [`ReplayStore::load_replay_nonces`] error-propagation path (site 2 of
    /// `2026-05-21-silent-failures-in-security-paths`). Pre-2026-05-23
    /// the loader silently filtered such rows via `.filter_map`, opening
    /// an ADR-029 replay-defense gap under storage corruption.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert_malformed_replay_nonce(
        &self,
        contact_id: &str,
        bad_nonce: &[u8],
        timestamp: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO replay_nonces (contact_id, nonce, timestamp)
             VALUES (?1, ?2, ?3)",
            params![contact_id, bad_nonce, timestamp as i64],
        )?;
        Ok(())
    }

    /// Checks whether a replay nonce has already been recorded for a contact.
    ///
    /// Returns `true` if the nonce exists (i.e., this is a replay), `false` if fresh.
    pub fn is_replay_nonce(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
    ) -> Result<bool, StorageError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM replay_nonces WHERE contact_id = ?1 AND nonce = ?2",
            params![contact_id, nonce.as_slice()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Loads all replay nonces for a contact.
    ///
    /// Returns (nonce, timestamp) pairs.
    pub fn load_replay_nonces(
        &self,
        contact_id: &str,
    ) -> Result<Vec<([u8; 32], u64)>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT nonce, timestamp FROM replay_nonces WHERE contact_id = ?1 ORDER BY timestamp",
        )?;

        // Site 2 of `2026-05-21-silent-failures-in-security-paths`: row
        // read errors and `nonce_vec.try_into().ok()?` both used to be
        // silently dropped via `.filter_map`. A corrupted nonce row would
        // produce an empty/short set, opening an ADR-029 replay-defense
        // window without surfacing the storage fault anywhere. Propagate
        // both classes of error instead — a single corrupted row aborts
        // the load with a typed `Err`, and the caller decides how to
        // recover (e.g. refuse to process inbound updates until storage
        // is repaired). Healthy storage is unaffected.
        let raw_rows: Vec<(Vec<u8>, i64)> = stmt
            .query_map(params![contact_id], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<_, _>>()?;

        let mut nonces = Vec::with_capacity(raw_rows.len());
        for (nonce_vec, ts) in raw_rows {
            let actual_len = nonce_vec.len();
            let nonce: [u8; 32] = nonce_vec.try_into().map_err(|_| {
                StorageError::Serialization(format!(
                    "replay_nonces row has malformed nonce: expected 32 bytes, got {actual_len}"
                ))
            })?;
            nonces.push((nonce, ts as u64));
        }

        Ok(nonces)
    }

    /// Removes replay nonces older than the given cutoff timestamp.
    ///
    /// Returns the number of nonces removed.
    pub fn cleanup_replay_nonces(&self, cutoff: u64) -> Result<usize, StorageError> {
        let removed = self.conn.execute(
            "DELETE FROM replay_nonces WHERE timestamp < ?1",
            params![cutoff as i64],
        )?;
        Ok(removed)
    }
}
