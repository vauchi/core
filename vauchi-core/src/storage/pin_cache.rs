// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`PinCacheStore`](super::PinCacheStore).

use super::{Storage, StorageError};
use crate::network::PinnedCertificate;

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
        self.pin_cache().save_pin_cache(relay_url, pins)
    }
    /// Load cached certificate pins for a relay URL.
    ///
    /// Returns `Ok(Some((pins, fetched_at)))` if cached pins exist,
    /// or `Ok(None)` if no entry exists for this relay.
    pub fn load_pin_cache(
        &self,
        relay_url: &str,
    ) -> Result<Option<(Vec<PinnedCertificate>, u64)>, StorageError> {
        self.pin_cache().load_pin_cache(relay_url)
    }
    /// Remove cached certificate pins for a relay URL.
    ///
    /// No-op if no entry exists for this relay.
    pub fn clear_pin_cache(&self, relay_url: &str) -> Result<(), StorageError> {
        self.pin_cache().clear_pin_cache(relay_url)
    }
}
