// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Genesis-decrypt rate-limit persistence view (ADR-068, migration v63).
//!
//! A genesis decrypt attempt derives Double Ratchet keys from `shared_key`
//! before any signature check (`exchange::genesis::GenesisEnvelope::open`), so
//! the receive path must bound how often it will try — per contact and across
//! all contacts — to cap pre-authentication CPU work. Counters are durable so a
//! process restart cannot reset an in-progress attack. A rate-limited blob is
//! never ACKed to the relay (retriable — plan §REVISION F6); the caller retains
//! the blob and retries after the window resets. The counters hold no secret
//! material, so this store needs no encryption key.

use rusqlite::{OptionalExtension, params};

use super::super::{Storage, StorageError};
use crate::clock::Clock;
use std::sync::Arc;

/// Rate-limit window length (seconds).
pub const GENESIS_WINDOW_SECS: u64 = 3600;

/// Maximum genesis decrypt attempts per contact per window. Twenty allows two
/// attempts from each of the maximum ten devices; ratification point — tune
/// from PII-free counters, never below the legitimate multi-device need.
pub const GENESIS_CONTACT_ATTEMPTS_PER_WINDOW: u32 = 20;

/// Maximum genesis decrypt attempts across all contacts per window. Bounds
/// aggregate CPU/DB work from a hostile contact set; ratification point.
pub const GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW: u32 = 256;

/// Scoped persistence view for genesis-decrypt rate limiting.
pub struct GenesisLimitStore<'a> {
    conn: &'a rusqlite::Connection,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for genesis-decrypt rate limiting.
    pub fn genesis_limits(&self) -> GenesisLimitStore<'_> {
        GenesisLimitStore {
            conn: &self.conn,
            clock: &self.clock,
        }
    }
}

impl GenesisLimitStore<'_> {
    /// Consume one genesis-decrypt budget unit for `contact_id`, charging both
    /// the per-contact and the global counter atomically. Returns `true` if the
    /// attempt is within both caps (counters incremented), `false` if either
    /// cap is exhausted for the current window (no increment). A window whose
    /// start is older than [`GENESIS_WINDOW_SECS`] resets before charging.
    pub fn consume_decrypt_budget(&self, contact_id: &str) -> Result<bool, StorageError> {
        let now = self.clock.unix_seconds();
        // Single-connection, single-threaded: a savepoint keeps the two-counter
        // charge atomic so a mid-charge failure never leaves one incremented.
        self.conn.execute_batch("SAVEPOINT genesis_budget")?;
        let result = (|| -> Result<bool, StorageError> {
            let contact_ok = self.charge_contact(contact_id, now)?;
            if !contact_ok {
                return Ok(false);
            }
            let global_ok = self.charge_global(now)?;
            Ok(global_ok)
        })();
        match result {
            // A denied attempt must not leave the counter charged, so roll the
            // whole charge back; an allowed one commits both increments.
            Ok(true) => {
                self.conn.execute_batch("RELEASE genesis_budget")?;
                Ok(true)
            }
            Ok(false) => {
                self.conn
                    .execute_batch("ROLLBACK TO genesis_budget; RELEASE genesis_budget")?;
                Ok(false)
            }
            Err(e) => {
                self.conn
                    .execute_batch("ROLLBACK TO genesis_budget; RELEASE genesis_budget")?;
                Err(e)
            }
        }
    }

    fn charge_contact(&self, contact_id: &str, now: u64) -> Result<bool, StorageError> {
        let row: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT window_start, attempts FROM genesis_decrypt_contact_limits
                     WHERE contact_id = ?1",
                params![contact_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let attempts = current_attempts(row, now);
        if attempts >= GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
            return Ok(false);
        }
        let (window_start, next) = charged(row, now);
        self.conn.execute(
            "INSERT INTO genesis_decrypt_contact_limits (contact_id, window_start, attempts)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(contact_id) DO UPDATE SET window_start = ?2, attempts = ?3",
            params![contact_id, window_start as i64, next as i64],
        )?;
        Ok(true)
    }

    /// Attempts charged to `contact_id` in the current window (0 if none or the
    /// window has aged out). Test seam for asserting rollback correctness.
    #[cfg(any(test, feature = "testing"))]
    pub fn contact_attempts_in_window(&self, contact_id: &str) -> Result<u32, StorageError> {
        let now = self.clock.unix_seconds();
        let row: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT window_start, attempts FROM genesis_decrypt_contact_limits
                     WHERE contact_id = ?1",
                params![contact_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(current_attempts(row, now))
    }

    /// Count one declined genesis re-seat: a sibling alert arrived while a
    /// `[0;32]` session existed (guarded invariant 1, ADR-064 Amendment
    /// 2026-07-24). PII-free global counter — it sizes how often the
    /// lost-primary population is exercised, informing F4 activation urgency.
    pub fn record_reseat_skip(&self) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO genesis_reseat_skips (singleton, skips) VALUES (1, 1)
                 ON CONFLICT(singleton) DO UPDATE SET skips = skips + 1",
            [],
        )?;
        Ok(())
    }

    /// Total genesis re-seats declined by the receive guard.
    pub fn reseat_skips(&self) -> Result<u64, StorageError> {
        let skips: Option<i64> = self
            .conn
            .query_row(
                "SELECT skips FROM genesis_reseat_skips WHERE singleton = 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(skips.map(|s| s.max(0) as u64).unwrap_or(0))
    }

    fn charge_global(&self, now: u64) -> Result<bool, StorageError> {
        let row: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT window_start, attempts FROM genesis_decrypt_global_limit
                     WHERE singleton = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let attempts = current_attempts(row, now);
        if attempts >= GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW {
            return Ok(false);
        }
        let (window_start, next) = charged(row, now);
        self.conn.execute(
            "INSERT INTO genesis_decrypt_global_limit (singleton, window_start, attempts)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET window_start = ?1, attempts = ?2",
            params![window_start as i64, next as i64],
        )?;
        Ok(true)
    }
}

/// Attempts already charged in the current window: zero once the stored window
/// has aged out. Saturating cast: this table is unencrypted, so a corrupted or
/// externally-tampered oversized `attempts` value clamps to the cap-blocking
/// `u32::MAX` (fail closed) rather than wrapping to a small value.
fn current_attempts(row: Option<(i64, i64)>, now: u64) -> u32 {
    match row {
        Some((start, attempts)) if !window_expired(start, now) => saturating_u32(attempts),
        _ => 0,
    }
}

/// The `(window_start, attempts)` to store after charging one unit — starting a
/// fresh window when the stored one has aged out.
fn charged(row: Option<(i64, i64)>, now: u64) -> (u64, u32) {
    match row {
        Some((start, attempts)) if !window_expired(start, now) => (
            start.max(0) as u64,
            saturating_u32(attempts).saturating_add(1),
        ),
        _ => (now, 1),
    }
}

/// A window is aged out when the configured span has elapsed, OR when the clock
/// has moved BACKWARD past the stored start — a backward wall-clock correction
/// is a discontinuity that must reset the window, otherwise an exhausted window
/// would stay exhausted (blocking genesis) until wall time caught back up.
fn window_expired(window_start: i64, now: u64) -> bool {
    let start = window_start.max(0) as u64;
    now < start || now.saturating_sub(start) >= GENESIS_WINDOW_SECS
}

fn saturating_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
