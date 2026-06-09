// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`OhttpCacheStore`](super::OhttpCacheStore).

use super::{Storage, StorageError};

impl Storage {
    /// Save or replace the cached OHTTP key for a relay URL.
    ///
    /// Records the current Unix-epoch time as `fetched_at`. If a cached
    /// key already exists for this relay, it is overwritten (upsert).
    pub fn save_ohttp_key(&self, relay_url: &str, key_bytes: &[u8]) -> Result<(), StorageError> {
        self.ohttp_cache().save_ohttp_key(relay_url, key_bytes)
    }
    /// Load the cached OHTTP key for a relay URL.
    ///
    /// Returns `Ok(Some((key_bytes, fetched_at)))` if a cached key exists,
    /// or `Ok(None)` if the cache has no entry for this relay.
    pub fn load_ohttp_key(&self, relay_url: &str) -> Result<Option<(Vec<u8>, u64)>, StorageError> {
        self.ohttp_cache().load_ohttp_key(relay_url)
    }
    /// Remove the cached OHTTP key for a relay URL.
    ///
    /// No-op if no entry exists for this relay.
    pub fn clear_ohttp_key(&self, relay_url: &str) -> Result<(), StorageError> {
        self.ohttp_cache().clear_ohttp_key(relay_url)
    }
}
