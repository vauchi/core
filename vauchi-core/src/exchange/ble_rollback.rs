// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Exchange Rollback
//!
//! Tracks pending BLE exchange data for atomic commit/rollback.
//! Neither side commits until both have decrypted and validated.

use std::collections::HashMap;

use super::ExchangeError;

/// Tracks pending BLE exchange data for atomic commit/rollback.
///
/// Records are stored by contact ID. On success, `commit` extracts the
/// data for persistence. On failure, `rollback` or `rollback_all` discards
/// pending state so neither side keeps partial data.
pub struct BleRollback {
    pending: HashMap<String, Vec<u8>>,
}

impl BleRollback {
    /// Creates a new empty rollback tracker.
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Records pending exchange data for a contact.
    pub fn record_pending(&mut self, contact_id: String, data: Vec<u8>) {
        self.pending.insert(contact_id, data);
    }

    /// Returns `true` if there is pending data for the given contact.
    pub fn has_pending(&self, contact_id: &str) -> bool {
        self.pending.contains_key(contact_id)
    }

    /// Commits pending data, removing and returning it.
    ///
    /// Returns `ExchangeError::InvalidState` if no pending data exists
    /// for the given contact ID.
    pub fn commit(&mut self, contact_id: &str) -> Result<Vec<u8>, ExchangeError> {
        self.pending.remove(contact_id).ok_or_else(|| {
            ExchangeError::InvalidState(format!("no pending BLE data for contact '{contact_id}'"))
        })
    }

    /// Rolls back (discards) pending data for a single contact.
    ///
    /// Returns `Ok(())` even if no pending data exists (idempotent).
    pub fn rollback(&mut self, contact_id: &str) -> Result<(), ExchangeError> {
        self.pending.remove(contact_id);
        Ok(())
    }

    /// Rolls back all pending data.
    pub fn rollback_all(&mut self) {
        self.pending.clear();
    }
}

impl Default for BleRollback {
    fn default() -> Self {
        Self::new()
    }
}
