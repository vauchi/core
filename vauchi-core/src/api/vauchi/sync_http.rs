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

/// Recommended interval (seconds) between scheduled sync ticks.
///
/// Frontends use this to configure their platform scheduler
/// (`BGTaskScheduler` on iOS, `WorkManager` on Android) so the
/// 15-minute cadence lives in core, not duplicated as a magic
/// constant on each platform. Audit
/// `2026-04-28-lifecycle-session-residue-umbrella` item P2-C.
pub const PERIODIC_SYNC_INTERVAL_SECONDS: u64 = 900;

/// Maximum number of retry attempts the platform scheduler should
/// configure for a failed periodic sync. Audit P2-C.
pub const PERIODIC_SYNC_MAX_RETRIES: u32 = 3;

use super::receive_routing::process_received_blobs;
use super::{Vauchi, VauchiSyncOutcome};
use crate::api::error::{VauchiError, VauchiResult};
use crate::api::sync_controller::SyncController;
use crate::contact::Contact;
use crate::network::mailbox_token::{batch_register_tokens, current_day_epoch};
use crate::network::{
    AckStatus, Acknowledgment, HttpTransport, HttpTransportAdapter, HttpTransportConfig,
    MessagePayload, OhttpClient, PinnedCertificate, RegisterMailbox, RelayClient, Transport,
    TransportConfig, create_envelope,
};

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
    #[tracing::instrument(level = "info", skip_all, name = "vauchi.sync")]
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
                // Key is stale — evict cache, re-resolve (bundled or direct), retry
                let relay_url = self.http_relay_url();
                let _ = self.storage.clear_ohttp_key(&relay_url);
                let key_bytes = self.resolve_ohttp_key(&relay_url)?;
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
    #[tracing::instrument(level = "debug", skip_all, name = "vauchi.sync_inner")]
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

        // Capture version policy from relay response headers before adapter is moved.
        let version_policy = adapter.last_version_policy();

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
            version_policy,
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

    /// Run one periodic sync tick — invoked by the platform
    /// scheduler (`BGTaskScheduler` / `WorkManager`).
    ///
    /// This is the single entry point that frontends call from
    /// their background-task handler. Core owns the per-tick
    /// behaviour:
    ///
    /// 1. If the instance is missing identity or OHTTP key, returns
    ///    the corresponding `VauchiSyncOutcome` variant — no
    ///    connect attempt (the user hasn't onboarded / connected
    ///    yet). The platform scheduler can use the result to
    ///    decide whether to keep firing.
    /// 2. Otherwise delegates to `Vauchi::sync()`. Throttle / retry
    ///    timing already lives on `next_sync_allowed`, so a tick
    ///    fired during a back-off window returns
    ///    `VauchiSyncOutcome::TooSoon` without doing work.
    ///
    /// Audit `2026-04-28-lifecycle-session-residue-umbrella`
    /// item P2-C — moves the per-tick decision into core so iOS
    /// `BackgroundSyncService` and Android `SyncWorker` shrink to
    /// a one-call wrapper. The scheduler-level constants
    /// ([`PERIODIC_SYNC_INTERVAL_SECONDS`] /
    /// [`PERIODIC_SYNC_MAX_RETRIES`]) live above so the cadence
    /// and retry policy match across platforms.
    pub fn periodic_sync_tick(&mut self) -> VauchiResult<VauchiSyncOutcome> {
        if self.identity.is_none() {
            return Ok(VauchiSyncOutcome::NoIdentity);
        }
        if self.ohttp_key.is_none() {
            return Ok(VauchiSyncOutcome::NotConnected);
        }
        self.sync()
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
    /// 2. Resolve OHTTP key: cached → bundled → direct fetch (if `allow_direct`).
    /// 3. Validate key via `OhttpClient::new()`.
    /// 4. Store in `self.ohttp_key`.
    ///
    /// No direct health check is performed — the first OHTTP sync request
    /// serves as an implicit reachability check, avoiding a direct HTTPS
    /// connection that would leak the client's IP address.
    pub fn connect(&mut self) -> VauchiResult<()> {
        // 1. Identity gate
        if self.identity.is_none() {
            return Err(VauchiError::IdentityNotInitialized);
        }

        // 2. Obtain OHTTP key bytes (cached → bundled → direct fetch)
        let relay_url = self.http_relay_url();
        let key_bytes = self.resolve_ohttp_key(&relay_url)?;

        // 3-4. Validate and store
        let client = OhttpClient::new(key_bytes).map_err(VauchiError::Network)?;
        self.ohttp_key = Some(client);

        Ok(())
    }

    /// Whether an OHTTP gateway key is currently cached on this instance.
    ///
    /// Used by platform wrappers and tests to verify that
    /// `connect()` populated the key — a prerequisite for
    /// `build_relay_transport` to wire OHTTP into downstream calls
    /// (device link, shred, exchange).
    pub fn has_ohttp_key(&self) -> bool {
        self.ohttp_key.is_some()
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
    #[tracing::instrument(level = "debug", skip_all, name = "sync.receive_phase")]
    fn run_receive_phase(
        &self,
        identity: &crate::identity::Identity,
        contacts: &[Contact],
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<usize> {
        // 1. Register mailbox tokens so the adapter knows what to fetch
        self.register_tokens(identity, contacts, adapter)?;

        // 2. Fetch all pending blobs (collect before processing).
        //    `mailbox_token_hex` is the hex-encoded daily-rotating token the
        //    blob arrived for, populated by the relay per ADR-029 addendum
        //    2026-04-27. Blobs whose token doesn't resolve to a known
        //    contact are dropped (post-Step 2: no brute-force fallback).
        let mut blobs: Vec<(String, String, Vec<u8>)> = Vec::new();
        while let Some(envelope) = adapter.receive().map_err(VauchiError::Network)? {
            if let MessagePayload::EncryptedUpdate(update) = envelope.payload {
                blobs.push((envelope.message_id, update.recipient_id, update.ciphertext));
            }
        }

        if blobs.is_empty() {
            return Ok(0);
        }

        // 3. Route + apply each blob, build per-blob ACK envelopes.
        let outcomes = process_received_blobs(identity, &self.storage, contacts, blobs);
        let received = outcomes.iter().filter(|o| o.decrypted).count();
        let rejected = outcomes
            .iter()
            .filter(|o| o.token_resolved && !o.decrypted)
            .count();
        let unresolved = outcomes.iter().filter(|o| !o.token_resolved).count();
        if unresolved > 0 {
            // Operational signal: a non-zero unresolved count after the
            // 2026-04-27 relay rollout indicates one of:
            //   - the relay regressed and stopped emitting mailbox_token,
            //   - we received a self-token blob (device-sync — handled
            //     separately, see receive_routing module doc),
            //   - clock drift beyond ±1 day, or
            //   - an attacker probing with random tokens.
            // No payload contents logged (logging-rules.md).
            tracing::warn!(
                unresolved,
                received,
                rejected,
                "sync.receive_phase: blobs with no contact-token match — investigate"
            );
        } else {
            tracing::debug!(received, rejected, "sync.receive_phase: token-routed");
        }

        // 4. Send ACK envelopes — best-effort, transport failures don't fail
        //    the receive cycle.
        for outcome in &outcomes {
            let ack_envelope = create_envelope(MessagePayload::Acknowledgment(Acknowledgment {
                message_id: outcome.message_id.clone(),
                status: if outcome.decrypted {
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
    #[tracing::instrument(level = "debug", skip_all, name = "sync.send_phase")]
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
            pinned_certs: self.resolve_pins(),
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

    /// Resolve the OHTTP key: cached → bundled → direct fetch (if allowed).
    ///
    /// Priority order:
    /// 1. Storage cache (if within TTL)
    /// 2. Bundled key from `OhttpConfig::bundled_gateway_key`
    /// 3. Direct fetch from relay (only if `allow_direct` is true)
    ///
    /// The bundled key eliminates the need for a direct HTTPS connection
    /// to the relay on first use, preventing client IP leakage.
    fn resolve_ohttp_key(&self, relay_url: &str) -> VauchiResult<Vec<u8>> {
        // 1. Try loading from cache — use if still within TTL
        if let Some((cached_bytes, fetched_at)) = self.storage.load_ohttp_key(relay_url)?
            && self.is_ohttp_key_fresh(fetched_at)
        {
            return Ok(cached_bytes);
        }

        // 2. Direct fetch (only for dev/testing — leaks client IP).
        //    Check before bundled key so test relays with ephemeral keys
        //    aren't shadowed by the compiled-in production key.
        if self.config.ohttp.allow_direct {
            return self.fetch_and_cache_ohttp_key(relay_url);
        }

        // 3. Try bundled key (no network, no IP leak)
        if let Some(ref bundled) = self.config.ohttp.bundled_gateway_key {
            // Validate the bundled key before using it — skip if corrupt
            if OhttpClient::new(bundled.clone()).is_ok() {
                return Ok(bundled.clone());
            }
            // Invalid bundled key — fall through to error
        }

        Err(VauchiError::Network(
            crate::network::NetworkError::ConnectionFailed(
                "no OHTTP key available: cache expired, no bundled key, direct fetch disabled"
                    .into(),
            ),
        ))
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
        let transport = self.create_bootstrap_transport_direct();
        let key_bytes = transport.fetch_ohttp_key().map_err(VauchiError::Network)?;
        self.storage.save_ohttp_key(relay_url, &key_bytes)?;
        Ok(key_bytes)
    }

    /// Resolve the effective certificate pin set for the relay.
    ///
    /// If `pin_config_verify_key` is configured, merges bundled pins with
    /// cached/fetched pins from the relay's `/v2/pin-config` endpoint.
    /// All remote pin responses must be Ed25519-signed by the relay operator.
    ///
    /// If `pin_config_verify_key` is `None` (default), returns only the
    /// bundled pins — no remote fetch is attempted.
    ///
    /// Returns at minimum the bundled pins — cache/network failures are non-fatal.
    pub(crate) fn resolve_pins(&self) -> Vec<PinnedCertificate> {
        let mut pins = self.config.relay.pinned_certs.clone();

        // Pin rotation requires a verify key — without it, only bundled pins are used
        let Some(ref verify_key) = self.config.relay.pin_config_verify_key else {
            return pins;
        };

        let relay_url = self.http_relay_url();

        // Try loading cached pins (best-effort — cache miss is fine)
        if let Ok(Some((cached_pins, fetched_at))) = self.storage.load_pin_cache(&relay_url)
            && self.is_pin_cache_fresh(fetched_at)
        {
            merge_pins(&mut pins, &cached_pins);
            return pins;
        }

        // Cache miss or stale — try refreshing (non-fatal on failure)
        if let Ok(refreshed) = self.refresh_pin_cache(&relay_url, verify_key) {
            merge_pins(&mut pins, &refreshed);
        }

        pins
    }

    /// Check whether cached pins are still within their TTL.
    fn is_pin_cache_fresh(&self, fetched_at_epoch_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age = now.saturating_sub(fetched_at_epoch_secs);
        age < self.config.relay.pin_ttl_secs
    }

    /// Fetch fresh signed pins from the relay and cache them in storage.
    fn refresh_pin_cache(
        &self,
        relay_url: &str,
        verify_key: &[u8; 32],
    ) -> VauchiResult<Vec<PinnedCertificate>> {
        let transport = self.create_bootstrap_transport_direct();
        let pins = transport
            .fetch_pin_config(verify_key)
            .map_err(VauchiError::Network)?;
        self.storage.save_pin_cache(relay_url, &pins)?;
        Ok(pins)
    }

    /// Create an `HttpTransport` in direct (non-OHTTP) mode for the
    /// bootstrap operations that cannot themselves go through OHTTP:
    /// fetching the gateway key, health checks, and pin-config refresh.
    ///
    /// This is the documented exception to the "all relay traffic flows
    /// through OHTTP" rule — see the §Bootstrap Exceptions section of
    /// `docs/docs/developers/threat-model.md`. The caller's IP is visible
    /// to the relay for these requests. In production, bundled keys
    /// (`OhttpConfig::bundled_gateway_key`) eliminate the OHTTP-key fetch
    /// entirely; pin-config refresh remains the infrequent exception.
    ///
    /// Uses only bundled pins (not `resolve_pins`) to avoid circular
    /// dependency: pin-config fetch must not depend on cached pins
    /// from a previous pin-config fetch.
    fn create_bootstrap_transport_direct(&self) -> HttpTransport {
        HttpTransport::new(HttpTransportConfig {
            relay_url: self.http_relay_url(),
            timeout_ms: self.config.relay.connect_timeout_ms,
            proxy: self.config.relay.proxy.clone(),
            allow_direct: true,
            pinned_certs: self.config.relay.pinned_certs.clone(),
        })
    }

    /// Build an `HttpTransport` to the given relay URL with OHTTP wired
    /// from the cached gateway key, if one is available.
    ///
    /// Used by call sites outside the sync path (device link, exchange,
    /// guardian, shred) that need a transport to a relay endpoint while
    /// still honoring ADR-037's IP-privacy guarantee. `allow_direct` is
    /// true only when `connect()` has not yet succeeded — once a key is
    /// cached, the transport fails closed on OHTTP failure instead of
    /// silently leaking the client's source IP.
    pub fn build_relay_transport(&self, relay_url: String, timeout_ms: u64) -> HttpTransport {
        let mut transport = HttpTransport::new(HttpTransportConfig {
            relay_url,
            timeout_ms,
            proxy: self.config.relay.proxy.clone(),
            allow_direct: self.ohttp_key.is_none(),
            pinned_certs: self.config.relay.pinned_certs.clone(),
        });

        if let Some(ref cached) = self.ohttp_key
            && let Ok(client) = OhttpClient::new(cached.encoded_config().to_vec())
        {
            transport.set_ohttp(client);
        }

        transport
    }

    /// Returns the relay URL for HTTP requests.
    pub(crate) fn http_relay_url(&self) -> String {
        self.config.relay.server_url.clone()
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

/// Merge `source` pins into `target`, skipping duplicates.
fn merge_pins(target: &mut Vec<PinnedCertificate>, source: &[PinnedCertificate]) {
    for pin in source {
        if !target.contains(pin) {
            target.push(pin.clone());
        }
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
        let (v, _dir) = vauchi_with_server_url("https://relay.example.com/ws");
        assert_eq!(
            v.http_relay_url(),
            "https://relay.example.com/ws",
            "https:// must be converted to https://"
        );
    }

    // @scenario: ohttp_sync :: URL scheme conversion ws to http
    #[test]
    fn test_http_relay_url_ws_converts_to_http() {
        let (v, _dir) = vauchi_with_server_url("http://relay.local/ws");
        assert_eq!(
            v.http_relay_url(),
            "http://relay.local/ws",
            "http:// must be converted to http://"
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

    // =========================================================================
    // W-5: merge_pins deduplication
    // =========================================================================

    // @scenario: pinning :: merge_pins deduplicates
    #[test]
    fn test_merge_pins_deduplicates() {
        let pin_a = PinnedCertificate::new([0xAA; 32]);
        let pin_b = PinnedCertificate::new([0xBB; 32]);

        let mut target = vec![pin_a.clone()];
        merge_pins(&mut target, &[pin_a.clone(), pin_b.clone()]);

        assert_eq!(target.len(), 2, "duplicate pin_a must not be added twice");
        assert_eq!(target[0], pin_a);
        assert_eq!(target[1], pin_b);
    }

    // @scenario: pinning :: merge_pins handles empty source
    #[test]
    fn test_merge_pins_empty_source() {
        let pin_a = PinnedCertificate::new([0xAA; 32]);
        let mut target = vec![pin_a.clone()];
        merge_pins(&mut target, &[]);
        assert_eq!(target.len(), 1, "empty source must not change target");
    }

    // @scenario: pinning :: merge_pins handles empty target
    #[test]
    fn test_merge_pins_empty_target() {
        let pin_a = PinnedCertificate::new([0xAA; 32]);
        let mut target = vec![];
        merge_pins(&mut target, std::slice::from_ref(&pin_a));
        assert_eq!(target.len(), 1, "source pins must be added to empty target");
        assert_eq!(target[0], pin_a);
    }

    // =========================================================================
    // W-6: is_pin_cache_fresh TTL logic
    // =========================================================================

    // @scenario: pinning :: fresh cache within TTL
    #[test]
    fn test_is_pin_cache_fresh_within_ttl() {
        let (v, _dir) = vauchi_with_server_url("https://relay.example.com");
        // Default pin_ttl_secs is 86400 (24h)
        assert_eq!(v.config.relay.pin_ttl_secs, 86_400);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(
            v.is_pin_cache_fresh(now - 100),
            "cache fetched 100s ago must be fresh (TTL=86400)"
        );
    }

    // @scenario: pinning :: stale cache beyond TTL
    #[test]
    fn test_is_pin_cache_stale_beyond_ttl() {
        let (v, _dir) = vauchi_with_server_url("https://relay.example.com");
        assert_eq!(v.config.relay.pin_ttl_secs, 86_400);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(
            !v.is_pin_cache_fresh(now - 90_000),
            "cache fetched 90000s ago must be stale (TTL=86400)"
        );
    }

    // @scenario: pinning :: resolve_pins returns only bundled when no verify key
    #[test]
    fn test_resolve_pins_returns_bundled_only_without_verify_key() {
        let (v, _dir) = vauchi_with_server_url("https://relay.example.com");
        assert!(
            v.config.relay.pin_config_verify_key.is_none(),
            "test assumes no verify key"
        );
        let pins = v.resolve_pins();
        assert_eq!(
            pins, v.config.relay.pinned_certs,
            "without verify key, resolve_pins must return only bundled pins"
        );
    }

    // @scenario: pinning :: resolve_pins falls back to bundled when refresh fails
    #[test]
    fn test_resolve_pins_falls_back_to_bundled_on_refresh_failure() {
        use crate::api::VauchiConfig;
        let dir = tempfile::tempdir().expect("tempdir must succeed");
        let mut cfg = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
            .with_relay_url("https://unreachable.invalid");
        // Set a verify key so resolve_pins attempts a remote fetch
        cfg.relay.pin_config_verify_key = Some([0x42; 32]);
        let v = Vauchi::new(cfg).expect("Vauchi::new must succeed");

        // No cache exists and relay is unreachable — refresh will fail
        let pins = v.resolve_pins();

        assert_eq!(
            pins, v.config.relay.pinned_certs,
            "on refresh failure, resolve_pins must return bundled pins (not empty)"
        );
        assert!(!pins.is_empty(), "bundled pins must not be empty");
    }
}
