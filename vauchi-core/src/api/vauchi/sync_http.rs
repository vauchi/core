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

use std::time::SystemTime;

use super::{Vauchi, VauchiSyncOutcome};
use crate::api::error::{VauchiError, VauchiResult};
use crate::api::sync_controller::SyncController;
use crate::network::mailbox_token::{batch_register_tokens, current_day_epoch};
use crate::network::message::{AckStatus, Acknowledgment, MessagePayload, RegisterMailbox};
use crate::network::protocol::create_envelope;
use crate::network::transport::{Transport, TransportConfig};
use crate::network::{
    HttpTransport, HttpTransportAdapter, HttpTransportConfig, OhttpClient, RelayClient,
    RelayClientConfig,
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
    /// 2. Create ephemeral `HttpTransportAdapter` with OHTTP encryption.
    /// 3. Connect adapter (relay health check).
    /// 4. **Receive phase**: register mailbox tokens, fetch blobs, decrypt + apply.
    /// 5. **Send phase**: delegate to `SyncController` for outgoing updates.
    /// 6. Update timing (stub — Task 10 implements C1/C2 jitter).
    /// 7. Return combined outcome.
    pub fn sync(&mut self) -> VauchiResult<VauchiSyncOutcome> {
        // 1. Gate checks
        let identity = match &self.identity {
            Some(id) => id,
            None => return Ok(VauchiSyncOutcome::NoIdentity),
        };

        let ohttp_key = match &self.ohttp_key {
            Some(key) => key,
            None => return Ok(VauchiSyncOutcome::NotConnected),
        };

        // TODO(Task 10): check next_sync_allowed — return TooSoon if too early

        // 2. Create ephemeral adapter with OHTTP key
        let mut adapter = self.create_ohttp_adapter(ohttp_key)?;

        // 3. Connect adapter (health check)
        adapter
            .connect(&TransportConfig::default())
            .map_err(VauchiError::Network)?;

        // 4. Receive phase
        let received = self.run_receive_phase(identity, &mut adapter)?;

        // 5. Send phase — adapter moves into RelayClient → SyncController
        let send_result = self.run_send_phase(identity, adapter)?;

        // 6. Update timing (stub for Task 10)
        self.update_timing_after_sync();

        // 7. Combine results
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
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<usize> {
        // 1. Register mailbox tokens so the adapter knows what to fetch
        self.register_tokens(identity, adapter)?;

        // 2. Fetch all pending blobs
        let mut ciphertexts: Vec<(String, Vec<u8>)> = Vec::new();
        loop {
            match adapter.receive().map_err(VauchiError::Network)? {
                Some(envelope) => {
                    if let MessagePayload::EncryptedUpdate(update) = envelope.payload {
                        // Queue ACK for this blob
                        let ack_envelope =
                            create_envelope(MessagePayload::Acknowledgment(Acknowledgment {
                                message_id: envelope.message_id,
                                status: AckStatus::ReceivedByRecipient,
                                error: None,
                            }));
                        let _ = adapter.send(&ack_envelope);

                        ciphertexts.push((update.sender_id, update.ciphertext));
                    }
                }
                None => break,
            }
        }

        if ciphertexts.is_empty() {
            return Ok(0);
        }

        // 3. For each ciphertext, try processing against each contact.
        //    process_single_card_update loads the ratchet from storage,
        //    attempts decryption, and only persists on success. Failed
        //    attempts are harmless (ratchet not advanced).
        let contacts = self.storage.list_contacts().unwrap_or_default();
        let contact_ids: Vec<String> = contacts
            .iter()
            .filter(|c| c.is_exchanged() && !c.is_blocked())
            .map(|c| c.id().to_string())
            .collect();

        let mut received = 0usize;
        for (_sender_id, ciphertext) in &ciphertexts {
            // Try each contact — the one whose ratchet decrypts successfully
            // is the sender. This is O(contacts * messages) but both numbers
            // are small for a personal contact app.
            for contact_id in &contact_ids {
                if process_single_card_update(identity, &self.storage, contact_id, ciphertext)
                    .is_ok()
                {
                    received += 1;
                    break;
                }
            }
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
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<()> {
        let contacts = self.storage.list_contacts().unwrap_or_default();

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
        adapter: HttpTransportAdapter,
    ) -> VauchiResult<crate::api::sync_controller::SyncResult> {
        let our_id = hex::encode(identity.signing_public_key());

        // Build RelayClient wrapping the adapter
        let relay = RelayClient::new(adapter, RelayClientConfig::default(), our_id);

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

        // Load ratchet states for all contacts
        let contacts = self.storage.list_contacts().unwrap_or_default();
        for contact in &contacts {
            if !contact.is_exchanged() || contact.is_blocked() {
                continue;
            }
            if let Ok(Some((ratchet, _is_initiator))) =
                self.storage.load_ratchet_state(contact.id())
            {
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
        ctrl.sync()
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

    /// Stub for timing update — Task 10 implements C1 + C2 jitter.
    fn update_timing_after_sync(&mut self) {
        // No-op: timing logic will be added in Task 10.
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
