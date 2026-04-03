// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OHTTP-aware connect/bootstrap for `Vauchi`.
//!
//! Implements `Vauchi::connect()` which:
//! 1. Validates that an identity exists.
//! 2. Loads or fetches the OHTTP gateway key.
//! 3. Validates the key via `OhttpClient::new()`.
//! 4. Caches the key in Storage and stores it in memory.
//! 5. Runs a health check against the relay.

use std::time::SystemTime;

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};
use crate::network::{HttpTransport, HttpTransportConfig, OhttpClient};

impl Vauchi {
    /// Connect to the relay, bootstrapping the OHTTP key.
    ///
    /// Must be called before `sync()`. Requires an identity to be created.
    ///
    /// # Flow
    ///
    /// 1. Check identity exists (fail with `IdentityNotInitialized` if not).
    /// 2. Load cached OHTTP key from storage.
    /// 3. If cached and fresh (age < `config.ohttp.key_ttl_secs`), reuse it.
    /// 4. Otherwise fetch from `GET /v2/ohttp-key` and cache.
    /// 5. Validate key via `OhttpClient::new()`.
    /// 6. Store in `self.ohttp_key`.
    /// 7. Run a health check to verify relay reachability.
    pub fn connect(&mut self) -> VauchiResult<()> {
        // 1. Identity gate
        if self.identity.is_none() {
            return Err(VauchiError::IdentityNotInitialized);
        }

        // 2-4. Obtain OHTTP key bytes (cached or freshly fetched)
        let relay_url = self.http_relay_url();
        let key_bytes = self.resolve_ohttp_key(&relay_url)?;

        // 5. Validate and store
        let client = OhttpClient::new(key_bytes).map_err(VauchiError::Network)?;
        self.ohttp_key = Some(client);

        // 6. Health check
        let transport = self.create_bare_transport();
        transport.health_check().map_err(VauchiError::Network)?;

        Ok(())
    }

    /// Resolve the OHTTP key: use cache if fresh, otherwise fetch and cache.
    fn resolve_ohttp_key(&self, relay_url: &str) -> VauchiResult<Vec<u8>> {
        // Try loading from cache — use if still within TTL
        if let Some((cached_bytes, fetched_at)) = self.storage.load_ohttp_key(relay_url)?
            && self.is_ohttp_key_fresh(fetched_at)
        {
            return Ok(cached_bytes);
        }
        // Cache miss or stale — fetch fresh key
        self.fetch_and_cache_ohttp_key(relay_url)
    }

    /// Check whether a cached OHTTP key is still within its TTL.
    fn is_ohttp_key_fresh(&self, fetched_at_epoch_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age = now.saturating_sub(fetched_at_epoch_secs);
        age < self.config.ohttp.key_ttl_secs
    }

    /// Fetch a fresh OHTTP key from the relay and cache it in storage.
    fn fetch_and_cache_ohttp_key(&self, relay_url: &str) -> VauchiResult<Vec<u8>> {
        let transport = self.create_bare_transport();
        let key_bytes = transport.fetch_ohttp_key().map_err(VauchiError::Network)?;
        self.storage.save_ohttp_key(relay_url, &key_bytes)?;
        Ok(key_bytes)
    }

    /// Create a bare `HttpTransport` (no OHTTP, direct allowed) for
    /// bootstrap operations: fetching the OHTTP key and health checks.
    fn create_bare_transport(&self) -> HttpTransport {
        HttpTransport::new(HttpTransportConfig {
            relay_url: self.http_relay_url(),
            timeout_ms: self.config.relay.connect_timeout_ms,
            proxy: self.config.relay.proxy.clone(),
            allow_direct: true,
        })
    }

    /// Derive the HTTP relay URL from the configured WebSocket URL.
    ///
    /// Converts `wss://` to `https://` and `ws://` to `http://`.
    /// Falls through unchanged for URLs that are already `http(s)://`.
    fn http_relay_url(&self) -> String {
        let url = &self.config.relay.server_url;
        if let Some(rest) = url.strip_prefix("wss://") {
            format!("https://{rest}")
        } else if let Some(rest) = url.strip_prefix("ws://") {
            format!("http://{rest}")
        } else {
            url.clone()
        }
    }
}
