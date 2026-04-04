// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OHTTP-aware connect/sync for `Vauchi`.
//!
//! Implements `Vauchi::connect()` and `Vauchi::sync()`.
//!
//! ## connect()
//! 1. Validates that an identity exists.
//! 2. Loads or fetches the OHTTP gateway key.
//! 3. Validates the key via `OhttpClient::new()`.
//! 4. Caches the key in Storage and stores it in memory.
//! 5. Runs a health check against the relay.
//!
//! ## sync()
//! 1. Gate checks: identity, OHTTP key, timing.
//! 2. Creates an ephemeral `HttpTransportAdapter` with OHTTP key.
//! 3. Connects the adapter (health check).
//! 4. Receive phase: register tokens, fetch blobs, decrypt + apply.
//! 5. Send phase: delegates to `SyncController` for outbound updates + ACKs.
//! 6. Returns combined `VauchiSyncOutcome`.

use std::time::{Duration, Instant, SystemTime};

use super::{Vauchi, VauchiSyncOutcome};
use crate::api::error::{VauchiError, VauchiResult};
use crate::api::sync_controller::SyncController;
use crate::contact::Contact;
use crate::network::mailbox_token::{batch_register_tokens, current_day_epoch};
use crate::network::{
    AckStatus, Acknowledgment, HttpTransport, HttpTransportAdapter, HttpTransportConfig,
    MessagePayload, OhttpClient, RegisterMailbox, RelayClient, Transport, TransportConfig,
    create_envelope,
};
use crate::sync::card_update::process_single_card_update;

impl Vauchi {
    /// Perform a bidirectional sync: receive pending messages, send outgoing updates.
    ///
    /// Must call `connect()` first to bootstrap the OHTTP key.
    ///
    /// # Flow
    ///
    /// 1. Gate checks (identity, OHTTP key, timing).
    /// 2. Attempt sync. On OHTTP key error, refresh key and retry once.
    /// 3. Update timing (C1/C2 jitter) on success.
    /// 4. Return combined outcome.
    pub fn sync(&mut self) -> VauchiResult<VauchiSyncOutcome> {
        // 1. Gate checks
        if self.identity.is_none() {
            return Ok(VauchiSyncOutcome::NoIdentity);
        }

        if self.ohttp_key.is_none() {
            return Ok(VauchiSyncOutcome::NotConnected);
        }

        // C1 / C2 timing gate
        if self
            .next_sync_allowed
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            return Ok(VauchiSyncOutcome::TooSoon);
        }

        // 2. Attempt sync, with one retry on stale OHTTP key
        match self.sync_inner() {
            Ok(outcome) => {
                self.update_timing_after_sync();
                Ok(outcome)
            }
            Err(ref e) if is_ohttp_key_error(e) => {
                // Key is stale — evict cache, re-fetch, retry once
                let relay_url = self.http_relay_url();
                let _ = self.storage.clear_ohttp_key(&relay_url);
                let key_bytes = self.fetch_and_cache_ohttp_key(&relay_url)?;
                let client = OhttpClient::new(key_bytes).map_err(VauchiError::Network)?;
                self.ohttp_key = Some(client);

                // Retry
                let outcome = self.sync_inner()?;
                self.update_timing_after_sync();
                Ok(outcome)
            }
            Err(e) => Err(e),
        }
    }

    /// Inner sync body — assumes gate checks already passed.
    ///
    /// Extracted so that `sync()` can retry on stale OHTTP key errors.
    fn sync_inner(&mut self) -> VauchiResult<VauchiSyncOutcome> {
        let identity = self.identity.as_ref().expect("identity checked in sync()");
        let ohttp_key = self
            .ohttp_key
            .as_ref()
            .expect("ohttp_key checked in sync()");

        // Load contacts once — shared by register_tokens, receive, and send phases.
        let contacts = self.storage.list_contacts().unwrap_or_default();

        // Create ephemeral adapter with OHTTP key
        let mut adapter = self.create_ohttp_adapter(ohttp_key)?;

        // Connect adapter (relay health check)
        adapter
            .connect(&TransportConfig::default())
            .map_err(VauchiError::Network)?;

        // Receive phase
        let received = self.run_receive_phase(identity, &contacts, &mut adapter)?;

        // Send phase — adapter moves into RelayClient → SyncController
        let send_result = self.run_send_phase(identity, &contacts, adapter)?;

        // Combine results
        let mut errors: Vec<String> = Vec::new();
        for (ctx, msg) in &send_result.errors {
            errors.push(format!("{ctx}: {msg}"));
        }

        Ok(VauchiSyncOutcome::Ok {
            received,
            sent: send_result.sent,
            acknowledged: send_result.acknowledged,
            errors,
        })
    }

    /// Set the post-exchange delay (C1).
    ///
    /// Call this after a successful in-person exchange. Computes a random delay
    /// in the range `[post_exchange_delay_min_ms, post_exchange_delay_max_ms]`
    /// and sets `next_sync_allowed` to `MAX(existing deadline, new deadline)`.
    ///
    /// Also records `last_exchange_time` for use by `update_timing_after_sync`.
    pub fn set_post_exchange_delay(&mut self) {
        let delay = self.config.sync.random_post_exchange_delay();
        let new_deadline = Instant::now() + delay;
        self.next_sync_allowed = Some(match self.next_sync_allowed {
            Some(existing) => existing.max(new_deadline),
            None => new_deadline,
        });
        self.last_exchange_time = Some(Instant::now());
    }

    /// Disconnect: clear the cached OHTTP key and sync timing state.
    ///
    /// After calling this, `sync()` returns `NotConnected` until `connect()` is
    /// called again.
    pub fn disconnect(&mut self) {
        self.ohttp_key = None;
        self.next_sync_allowed = None;
    }

    /// Test helper: directly set the `next_sync_allowed` deadline.
    #[cfg(any(test, feature = "testing"))]
    pub fn set_next_sync_allowed(&mut self, deadline: Instant) {
        self.next_sync_allowed = Some(deadline);
    }

    /// Test helper: inject a pre-built OHTTP key so tests can reach the
    /// timing gates without calling `connect()` against a real relay.
    ///
    /// Only available with the `testing` feature.
    #[cfg(feature = "testing")]
    pub fn set_ohttp_key_for_testing(&mut self, client: OhttpClient) {
        self.ohttp_key = Some(client);
    }

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

    // =====================================================================
    // Receive phase
    // =====================================================================

    /// Receive phase: register mailbox tokens, fetch blobs, decrypt + apply.
    ///
    /// For each fetched blob, tries decrypting with each contact's ratchet
    /// via `process_single_card_update`. The function that succeeds identifies
    /// the sender. Failed attempts are safe because they return early before
    /// advancing ratchet state.
    ///
    /// Returns the number of successfully received (decrypted + applied) updates.
    fn run_receive_phase(
        &self,
        identity: &crate::identity::Identity,
        contacts: &[Contact],
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<usize> {
        // 1. Register mailbox tokens so the adapter knows what to fetch
        self.register_tokens(identity, contacts, adapter)?;

        // 2. Fetch all pending blobs (collect before processing)
        let mut blobs: Vec<(String, String, Vec<u8>)> = Vec::new(); // (message_id, sender_id, ciphertext)
        while let Some(envelope) = adapter.receive().map_err(VauchiError::Network)? {
            if let MessagePayload::EncryptedUpdate(update) = envelope.payload {
                blobs.push((envelope.message_id, update.sender_id, update.ciphertext));
            }
        }

        if blobs.is_empty() {
            return Ok(0);
        }

        // 3. For each blob: try decrypt → apply → ACK.
        //    ACK is sent AFTER decryption attempt, not before.
        //    If no ratchet matches, ACK anyway to prevent infinite redelivery.
        let contact_ids: Vec<String> = contacts
            .iter()
            .filter(|c| c.is_exchanged() && !c.is_blocked())
            .map(|c| c.id().to_string())
            .collect();

        let mut received = 0usize;
        for (message_id, _sender_id, ciphertext) in &blobs {
            // Try each contact — the one whose ratchet decrypts successfully
            // is the sender. This is O(contacts × messages) but both numbers
            // are small for a personal contact app.
            let mut decrypted = false;
            for contact_id in &contact_ids {
                if process_single_card_update(identity, &self.storage, contact_id, ciphertext)
                    .is_ok()
                {
                    received += 1;
                    decrypted = true;
                    break;
                }
            }

            // ACK after attempting decryption — whether it succeeded or not.
            // Success: message processed, relay can discard.
            // Failure: undecryptable message, ACK prevents infinite redelivery.
            let ack_envelope = create_envelope(MessagePayload::Acknowledgment(Acknowledgment {
                message_id: message_id.clone(),
                status: if decrypted {
                    AckStatus::ReceivedByRecipient
                } else {
                    AckStatus::Stored // best-effort ACK for undecryptable
                },
                error: None,
            }));
            let _ = adapter.send(&ack_envelope);
        }

        Ok(received)
    }

    /// Register mailbox tokens on the adapter for fetch routing.
    ///
    /// Computes contact tokens from shared keys and a self-token from the
    /// master seed, registers them via `RegisterMailbox` messages. Tokens
    /// are padded to 256 per batch and shuffled to prevent relay inference.
    fn register_tokens(
        &self,
        identity: &crate::identity::Identity,
        contacts: &[Contact],
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<()> {
        // Collect shared keys from exchanged contacts
        let contact_keys: Vec<[u8; 32]> = contacts
            .iter()
            .filter_map(|c| c.shared_key().map(|k| *k.as_bytes()))
            .collect();

        let day = current_day_epoch();
        let master_seed = identity.master_seed();

        // Build padded token batches (256 per batch, shuffled)
        let batches = batch_register_tokens(&contact_keys, master_seed, day, 0);

        // Register each batch with the adapter
        for tokens in batches {
            let envelope =
                create_envelope(MessagePayload::RegisterMailbox(RegisterMailbox { tokens }));
            adapter.send(&envelope).map_err(VauchiError::Network)?;
        }

        Ok(())
    }

    // =====================================================================
    // Send phase
    // =====================================================================

    /// Send phase: delegate to SyncController for outgoing updates + ACKs.
    ///
    /// Moves the adapter into a `RelayClient` which is wrapped by a
    /// `SyncController`. Contact ratchets are loaded from storage and
    /// registered on the controller before calling `sync()`.
    fn run_send_phase(
        &self,
        identity: &crate::identity::Identity,
        contacts: &[Contact],
        adapter: HttpTransportAdapter,
    ) -> VauchiResult<crate::api::sync_controller::SyncResult> {
        let our_id = hex::encode(identity.signing_public_key());

        // Build RelayClient wrapping the adapter
        let relay_config = self.config.relay.to_relay_client_config(
            self.config.delivery_receipts_enabled,
            self.config.suppress_presence,
        );
        let relay = RelayClient::new(adapter, relay_config, our_id);

        // Build SyncController
        let mut ctrl = SyncController::new(
            relay,
            &self.storage,
            self.config.sync.clone(),
            self.events.clone(),
        );

        // Connect the relay (adapter is already connected from receive phase)
        // SyncController.connect() calls relay.connect() which calls
        // adapter.connect() — but the adapter is already connected, so the
        // health check runs again. This is fine for correctness.
        ctrl.connect()?;

        // Load ratchet states for all contacts, preserving is_initiator for save-back
        let mut initiator_flags: std::collections::HashMap<String, bool> =
            std::collections::HashMap::new();
        for contact in contacts {
            if !contact.is_exchanged() || contact.is_blocked() {
                continue;
            }
            if let Ok(Some((ratchet, is_initiator))) = self.storage.load_ratchet_state(contact.id())
            {
                initiator_flags.insert(contact.id().to_string(), is_initiator);
                ctrl.register_ratchet(contact.id(), ratchet);
            }
        }

        // Register mailbox tokens on the relay client for outbound routing
        let contact_keys: Vec<[u8; 32]> = contacts
            .iter()
            .filter_map(|c| c.shared_key().map(|k| *k.as_bytes()))
            .collect();
        let _ = ctrl.relay_mut().register_mailbox_tokens(
            &contact_keys,
            identity.master_seed(),
            0, // days_offline — Task 10 will compute this from last_connected_epoch
        );

        // Run the sync cycle (sends pending updates, processes ACKs)
        let result = ctrl.sync()?;

        // Persist advanced ratchet states.
        // SyncController.sync() advances ratchets via .encrypt() but
        // does not save them. Without this, ratchet state is lost on
        // drop, causing desync on next sync cycle.
        let ratchets = ctrl.into_ratchets();
        for (contact_id, ratchet) in &ratchets {
            let is_initiator = initiator_flags
                .get(contact_id.as_str())
                .copied()
                .unwrap_or(false);
            let _ = self
                .storage
                .save_ratchet_state(contact_id, ratchet, is_initiator);
        }

        Ok(result)
    }

    // =====================================================================
    // Helpers
    // =====================================================================

    /// Create an `HttpTransportAdapter` with OHTTP encryption from the
    /// cached key, or direct mode if OHTTP is unavailable and direct is allowed.
    ///
    /// Constructs a fresh `OhttpClient` from the cached encoded config bytes
    /// because `OhttpClient` is not `Clone` (it wraps single-use HPKE state).
    fn create_ohttp_adapter(&self, ohttp_key: &OhttpClient) -> VauchiResult<HttpTransportAdapter> {
        let adapter_ohttp =
            OhttpClient::new(ohttp_key.encoded_config().to_vec()).map_err(VauchiError::Network)?;
        let mut transport = HttpTransport::new(HttpTransportConfig {
            relay_url: self.http_relay_url(),
            timeout_ms: self.config.relay.connect_timeout_ms,
            proxy: self.config.relay.proxy.clone(),
            allow_direct: self.config.ohttp.allow_direct,
        });
        transport.set_ohttp(adapter_ohttp);
        Ok(HttpTransportAdapter::new(transport))
    }

    /// Update sync timing after a successful sync (C1 + C2).
    ///
    /// Computes the C2 deadline using a jittered sync interval. If the last
    /// exchange was recent (within `post_exchange_delay_max_ms`), a C1 deadline
    /// is also computed and the MAX of C1 and C2 is used.
    fn update_timing_after_sync(&mut self) {
        let c2_deadline = Instant::now() + self.config.sync.jittered_sync_interval();

        let deadline = if let Some(exchange_time) = self.last_exchange_time {
            let max_delay = Duration::from_millis(self.config.sync.post_exchange_delay_max_ms);
            if exchange_time.elapsed() < max_delay {
                // Exchange was recent — enforce C1 as well
                let c1_deadline = exchange_time + self.config.sync.random_post_exchange_delay();
                c1_deadline.max(c2_deadline)
            } else {
                c2_deadline
            }
        } else {
            c2_deadline
        };

        self.next_sync_allowed = Some(deadline);
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
    pub(crate) fn http_relay_url(&self) -> String {
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

/// Heuristic: does this error look like a stale/rejected OHTTP key?
///
/// Checks for HTTP 400 responses or error messages containing "ohttp" —
/// common signals when the relay rejects an outdated OHTTP config.
/// A false positive causes one extra key fetch (cheap). A false negative
/// means the caller sees a transient error and retries on the next sync.
fn is_ohttp_key_error(err: &VauchiError) -> bool {
    if let VauchiError::Network(ne) = err {
        let msg = ne.to_string().to_lowercase();
        msg.contains("400") || msg.contains("ohttp")
    } else {
        false
    }
}

// INLINE_TEST_REQUIRED: is_ohttp_key_error and http_relay_url() are private
// free functions / methods — they cannot be reached from tests/ without making
// them pub(crate). Inline tests are the least-invasive way to cover them.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::NetworkError;
    use crate::storage::StorageError;

    // =========================================================================
    // W-3: is_ohttp_key_error heuristic
    // =========================================================================

    // @scenario: ohttp_sync :: key error heuristic matches HTTP 400
    #[test]
    fn test_is_ohttp_key_error_http_400() {
        let err = VauchiError::Network(NetworkError::ConnectionFailed(
            "400 Bad Request".to_string(),
        ));
        assert!(
            is_ohttp_key_error(&err),
            "Network error containing '400' must be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic matches ohttp keyword
    #[test]
    fn test_is_ohttp_key_error_ohttp_in_message() {
        let err = VauchiError::Network(NetworkError::RelayRejected(
            "ohttp decapsulation failed".to_string(),
        ));
        assert!(
            is_ohttp_key_error(&err),
            "Network error containing 'ohttp' must be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic rejects storage errors
    #[test]
    fn test_is_ohttp_key_error_storage_error_is_false() {
        let err = VauchiError::Storage(StorageError::NotFound("key".to_string()));
        assert!(
            !is_ohttp_key_error(&err),
            "Storage error must NOT be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic rejects connection refused
    #[test]
    fn test_is_ohttp_key_error_connection_refused_is_false() {
        let err = VauchiError::Network(NetworkError::ConnectionFailed(
            "connection refused".to_string(),
        ));
        assert!(
            !is_ohttp_key_error(&err),
            "Connection-refused error must NOT be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic rejects timeout
    #[test]
    fn test_is_ohttp_key_error_timeout_is_false() {
        let err = VauchiError::Network(NetworkError::Timeout);
        assert!(
            !is_ohttp_key_error(&err),
            "Timeout error must NOT be classified as an OHTTP key error"
        );
    }

    // =========================================================================
    // W-4: http_relay_url() scheme conversion
    // =========================================================================

    fn vauchi_with_server_url(url: &str) -> (Vauchi, tempfile::TempDir) {
        use crate::api::VauchiConfig;
        let dir = tempfile::tempdir().expect("tempdir must succeed");
        let cfg = VauchiConfig::with_storage_path(dir.path().join("vauchi.db")).with_relay_url(url);
        let v = Vauchi::new(cfg).expect("Vauchi::new must succeed");
        (v, dir)
    }

    // @scenario: ohttp_sync :: URL scheme conversion wss to https
    #[test]
    fn test_http_relay_url_wss_converts_to_https() {
        let (v, _dir) = vauchi_with_server_url("wss://relay.example.com/ws");
        assert_eq!(
            v.http_relay_url(),
            "https://relay.example.com/ws",
            "wss:// must be converted to https://"
        );
    }

    // @scenario: ohttp_sync :: URL scheme conversion ws to http
    #[test]
    fn test_http_relay_url_ws_converts_to_http() {
        let (v, _dir) = vauchi_with_server_url("ws://relay.local/ws");
        assert_eq!(
            v.http_relay_url(),
            "http://relay.local/ws",
            "ws:// must be converted to http://"
        );
    }

    // @scenario: ohttp_sync :: URL scheme passthrough for https
    #[test]
    fn test_http_relay_url_https_unchanged() {
        let (v, _dir) = vauchi_with_server_url("https://relay.example.com");
        assert_eq!(
            v.http_relay_url(),
            "https://relay.example.com",
            "https:// must pass through unchanged"
        );
    }

    // @scenario: ohttp_sync :: URL scheme passthrough for http
    #[test]
    fn test_http_relay_url_http_unchanged() {
        let (v, _dir) = vauchi_with_server_url("http://relay.local");
        assert_eq!(
            v.http_relay_url(),
            "http://relay.local",
            "http:// must pass through unchanged"
        );
    }
}
