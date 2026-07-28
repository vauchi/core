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
//! 5. Send phase: delegates to `SendPhase` for outbound updates + ACKs.
//! 6. Returns combined `VauchiSyncOutcome`.

use std::time::Duration;
use url::{Origin, Url};

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

use super::ohttp_key_error::should_refetch_key_and_retry;
use super::receive_routing::{incoming_update_events, process_received_blobs};
use super::{Vauchi, VauchiSyncOutcome};
use crate::api::error::{VauchiError, VauchiResult};
use crate::api::send_phase::SendPhase;
use crate::contact::Contact;
use crate::network::{
    AckStatus, Acknowledgment, HttpTransport, HttpTransportAdapter, HttpTransportConfig,
    MessagePayload, OhttpClient, PinnedCertificate, RelayClient, Transport, TransportConfig,
    create_envelope,
};
use crate::rng::SecureRngExt;

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
        if self.identity.is_none() {
            return Ok(VauchiSyncOutcome::NoIdentity);
        }

        if self.ohttp_key.is_none() {
            return Ok(VauchiSyncOutcome::NotConnected);
        }

        // C1 / C2 timing gate
        if self
            .next_sync_allowed
            .is_some_and(|deadline| self.monotonic.now() < deadline)
        {
            return Ok(VauchiSyncOutcome::TooSoon);
        }

        // 2. Attempt sync, with one retry on a stale OHTTP key. A stale-key
        // failure surfaces on EITHER leg: the receive leg returns a typed
        // `Err`, the send leg folds it into `Ok { errors }`. Both must reach
        // the evict+refetch+retry path, otherwise a send-leg 502 is deferred a
        // full sync cadence (2026-05-25-relay-ohttp-forward-hop-502,
        // send-phase swallow).
        let first = self.sync_inner();
        if should_refetch_key_and_retry(&first) {
            // Key is stale — evict cache, re-resolve (bundled or direct), retry
            // once. best-effort: if clear fails the next sync cycle hits the
            // same stale-key error and retries this same path.
            let relay_url = self.http_relay_url();
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.storage.ohttp_cache().clear_ohttp_key(&relay_url);
            let key_bytes = self.resolve_ohttp_key(&relay_url)?;
            let client = OhttpClient::new(key_bytes).map_err(VauchiError::Network)?;
            self.ohttp_key = Some(client);

            let outcome = self.sync_inner()?;
            self.update_timing_after_sync();
            return Ok(outcome);
        }
        match first {
            Ok(outcome) => {
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
        // Load contacts once — shared by register_tokens, receive, and send phases.
        let contacts = self.storage.contacts().list_contacts().unwrap_or_default();

        // Receive/send run over the same hardened OHTTP transport as every
        // application-relay action (Step 4): build_relay_transport derives the
        // outer OHTTP hop internally and fails closed without it. sync() has
        // already returned NotConnected unless self.ohttp_key is Some.
        let mut adapter = HttpTransportAdapter::new(self.build_relay_transport(
            &self.config.relay.server_url,
            self.config.relay.connect_timeout_ms,
        ));

        // Connect adapter (relay health check)
        adapter
            .connect(&TransportConfig::default())
            .map_err(VauchiError::Network)?;

        let (received, fetched, rejected, unresolved, reject_reasons) =
            self.run_receive_phase(identity, &contacts, &mut adapter)?;

        // Capture version policy from relay response headers before adapter is moved.
        let version_policy = adapter.last_version_policy();

        // Queue any owed own-card repropagation (group-aware, idempotent against
        // each contact's baseline) so the send phase below delivers it in the
        // same tick. Best-effort — the durable marker retries on failure.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.run_owed_repropagation();

        // Queue reciprocity confirmations (P3 Slice B) for still-Pending
        // confirmable contacts so the send phase below delivers them this tick.
        // Ordered AFTER the receive phase: a confirm we just received may have
        // flipped a contact to Confirmed, and the Pending gate then excludes it —
        // so a mutually-confirmed pair stops re-sending (convergence). Best-effort.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.queue_reciprocity_confirmations();

        // Queue F4 registry pushes (vouched push, ADR-064 Amendment
        // 2026-07-25) for contacts that have not confirmed our current
        // registry version. AFTER the receive phase for the same reason as
        // reciprocity: a just-received ack may have confirmed a contact,
        // and the scanner then skips it (convergence). Best-effort.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.queue_registry_pushes();

        // Send phase — adapter moves into RelayClient → SendPhase
        let send_result = self.run_send_phase(identity, &contacts, adapter)?;

        let mut errors: Vec<String> = Vec::new();
        for (ctx, msg) in &send_result.errors {
            errors.push(format!("{ctx}: {msg}"));
        }

        Ok(VauchiSyncOutcome::Ok {
            received,
            fetched,
            rejected,
            unresolved,
            reject_reasons,
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
        let delay = self
            .config
            .sync
            .random_post_exchange_delay(self.rng.as_ref());
        let new_deadline = self.monotonic.now() + delay;
        self.next_sync_allowed = Some(match self.next_sync_allowed {
            Some(existing) => existing.max(new_deadline),
            None => new_deadline,
        });
        self.last_exchange_time = Some(self.monotonic.now());
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
    pub fn set_next_sync_allowed(&mut self, deadline: std::time::Instant) {
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
        let relay_url = self.distinct_ohttp_route().ok_or_else(ohttp_route_error)?;
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
    /// Returns `(received, fetched, rejected, unresolved)`: applied updates,
    /// total blobs fetched from the mailbox, token-resolved-but-undecryptable,
    /// and token-unresolved. The breakdown splits sent-not-received into
    /// delivery (`fetched=0`) vs decrypt/token (`fetched>0`) — diagnostics for
    /// 2026-06-28-sync-delivery-sent-not-received.
    /// Drains the mailbox across paginated fetches within a single receive
    /// phase. `/v2/fetch` caps each page under the OHTTP forward limit and
    /// flags `truncated`; after ACK-removing a page the next fetch returns the
    /// remainder (ADR-064 F4 fan-out can exceed the 128 KiB cap). The loop is
    /// bounded so a page that ACKs nothing (persistent storage failure) cannot
    /// spin forever.
    #[tracing::instrument(level = "debug", skip_all, name = "sync.receive_phase")]
    fn run_receive_phase(
        &self,
        identity: &crate::identity::Identity,
        contacts: &[Contact],
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<(usize, usize, usize, usize, String)> {
        const MAX_RECEIVE_PAGES: usize = 64;
        let (mut received, mut fetched, mut rejected, mut unresolved) = (0, 0, 0, 0);
        let mut reject_reasons: Vec<String> = Vec::new();
        for _ in 0..MAX_RECEIVE_PAGES {
            let (r, f, rej, unr, reasons) = self.run_receive_page(identity, contacts, adapter)?;
            received += r;
            fetched += f;
            rejected += rej;
            unresolved += unr;
            if !reasons.is_empty() {
                reject_reasons.push(reasons);
            }
            // Stop when the page was empty or the relay returned everything;
            // otherwise ACKs for this page are queued, so re-arm and re-fetch —
            // the next receive() flushes them before polling the remainder.
            if f == 0 || !adapter.last_fetch_truncated() {
                break;
            }
            adapter.allow_refetch();
        }
        Ok((
            received,
            fetched,
            rejected,
            unresolved,
            reject_reasons.join(","),
        ))
    }

    #[tracing::instrument(level = "debug", skip_all, name = "sync.receive_page")]
    fn run_receive_page(
        &self,
        identity: &crate::identity::Identity,
        contacts: &[Contact],
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<(usize, usize, usize, usize, String)> {
        // 1. Register mailbox tokens so the adapter knows what to fetch
        self.register_tokens(identity, contacts, adapter)?;

        // 2. Fetch all pending blobs, classifying each ONCE at the fetch
        //    site (`InboundBlob` is the single discriminator — Step 3 of
        //    the consolidation plan replaced the token-set membership and
        //    magic-prefix probes that were scattered downstream).
        //    `mailbox_token_hex` is the hex-encoded daily-rotating token the
        //    blob arrived for, populated by the relay per ADR-029 addendum
        //    2026-04-27. Blobs whose token doesn't resolve to a known
        //    contact are dropped (post-Step 2: no brute-force fallback).
        let self_tokens = self.self_token_hexes(identity);
        let mut fetched = 0usize;
        let mut device_blobs: Vec<(String, Vec<u8>)> = Vec::new();
        let mut revocations: Vec<(String, crate::network::IdentityRevoked)> = Vec::new();
        let mut update_blobs: Vec<(String, String, Vec<u8>, Option<String>)> = Vec::new();
        while let Some(envelope) = adapter.receive().map_err(VauchiError::Network)? {
            if let MessagePayload::EncryptedUpdate(update) = envelope.payload {
                fetched += 1;
                let origin_hint = update.origin_hint;
                match classify_inbound_blob(
                    &self_tokens,
                    envelope.message_id.into_string(),
                    update.recipient_id.into_string(),
                    update.ciphertext,
                ) {
                    InboundBlob::DeviceSync {
                        message_id,
                        ciphertext,
                    } => device_blobs.push((message_id, ciphertext)),
                    InboundBlob::Revocation {
                        message_id,
                        revocation,
                    } => revocations.push((message_id, revocation)),
                    InboundBlob::ContactUpdate {
                        message_id,
                        token,
                        ciphertext,
                    } => update_blobs.push((message_id, token, ciphertext, origin_hint)),
                }
            }
        }

        if fetched == 0 {
            // Still surface leftover durable alerts (e.g. from a session that
            // crashed between receive-commit and dispatch) — the empty-fetch
            // fast path must not starve them until new traffic arrives.
            if let Err(error) = self.surface_pending_safety_alerts() {
                tracing::warn!(?error, "sync.receive_phase: surfacing safety alerts failed");
            }
            return Ok((0, 0, 0, 0, String::new()));
        }

        // 2b. Device-sync blobs are sealed for the shared identity, not a
        //     contact, so the contact router cannot decrypt them. Apply +
        //     ACK each.
        let mut device_applied = 0usize;
        for (message_id, ciphertext) in &device_blobs {
            device_applied += self
                .apply_device_sync_blob(identity, ciphertext)
                .unwrap_or(0);
            let ack = create_envelope(
                MessagePayload::Acknowledgment(Acknowledgment {
                    message_id: message_id.clone().into(),
                    status: AckStatus::Stored,
                    error: None,
                }),
                self.clock.unix_seconds(),
                self.rng.uuid_v4().into(),
            );
            #[allow(clippy::let_underscore_must_use)]
            let _ = adapter.send(&ack);
        }

        // 2c. Signed identity-revocation blobs go to process_revocation,
        //     which verifies the signature against the stored contact and is
        //     a no-op on every failure path (unknown/stale/forged) — so a
        //     forged or garbage revocation cannot delete a contact.
        for (message_id, rev) in revocations {
            // ACK (let the relay drop the blob) only when processing did not hit
            // a storage error: a transient failure must NOT be ACKed so a later
            // sync retries, while a verified no-op and a successful shred both
            // ACK. Otherwise a WAL-lock/disk-full would silently lose the
            // revocation.
            if crate::network::revocation::process_revocation(&rev, &self.storage).is_err() {
                continue;
            }
            let ack = create_envelope(
                MessagePayload::Acknowledgment(Acknowledgment {
                    message_id: message_id.into(),
                    status: AckStatus::Stored,
                    error: None,
                }),
                self.clock.unix_seconds(),
                self.rng.uuid_v4().into(),
            );
            #[allow(clippy::let_underscore_must_use)]
            let _ = adapter.send(&ack);
        }

        // 3. Route + apply each contact blob, build per-blob ACK envelopes.
        let outcomes = process_received_blobs(identity, &self.storage, contacts, update_blobs);

        // 3b. Persist peer-card fan-out before acknowledging the relay blob.
        // If queue persistence fails, return without ACK so the relay retries.
        // A retry is classified as `replay`; rebuilding the snapshot in that
        // case closes the apply-then-queue crash window without re-emitting an
        // IncomingUpdate event.
        for outcome in &outcomes {
            if let Some(contact_id) = outcome.device_fanout_contact_id.as_deref() {
                self.record_received_contact_card_update(contact_id)?;
            }
        }

        // 3c. Queue F4 handshake ack replies (ADR-064 Amendment 2026-07-25).
        // A failed queue must not fail the receive phase: the handshake is
        // retry-tolerant — the peer re-pushes and this ack is re-requested.
        for outcome in &outcomes {
            if let Some(reply) = outcome.registry_reply.as_ref()
                && let Err(error) = self.queue_registry_ack(reply)
            {
                tracing::warn!("registry ack queue failed: {error}");
            }
        }

        let received = outcomes.iter().filter(|o| o.decrypted).count();
        let rejected = outcomes
            .iter()
            .filter(|o| o.token_resolved && !o.decrypted)
            .count();
        let unresolved = outcomes.iter().filter(|o| !o.token_resolved).count();
        // Per-category reject tally (PII-free, sorted) so the device can name
        // WHICH receive step failed when card updates don't apply
        // (2026-06-28-sync-delivery-sent-not-received). E.g. "decrypt:2".
        let reject_reasons = {
            let mut tally: std::collections::BTreeMap<&'static str, usize> =
                std::collections::BTreeMap::new();
            for outcome in &outcomes {
                if let Some(reason) = outcome.reject_reason {
                    *tally.entry(reason).or_insert(0) += 1;
                }
            }
            tally
                .iter()
                .map(|(cat, n)| format!("{cat}:{n}"))
                .collect::<Vec<_>>()
                .join(",")
        };
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
        for outcome in outcomes.iter().filter(|o| o.should_ack()) {
            let ack_envelope = create_envelope(
                MessagePayload::Acknowledgment(Acknowledgment {
                    message_id: outcome.message_id.clone().into(),
                    status: if outcome.decrypted {
                        AckStatus::ReceivedByRecipient
                    } else {
                        AckStatus::Stored // best-effort ACK for undecryptable
                    },
                    error: None,
                }),
                self.clock.unix_seconds(),
                self.rng.uuid_v4().into(),
            );
            // best-effort: ACK delivery; the relay will refetch on next
            // sync cycle if this batch's ACK is lost in flight
            #[allow(clippy::let_underscore_must_use)]
            let _ = adapter.send(&ack_envelope);
        }

        // 5. Invalidate the contacts list / contact-detail screens for each
        //    applied peer card update so the frontend reloads and renders the
        //    synced tile. Without this the list only refreshes on the next
        //    navigation — the 2026-06-30 "S7 synced tile not rendered" symptom
        //    (ADR-021/043; affected_screens maps IncomingUpdate -> contacts).
        for event in incoming_update_events(&outcomes) {
            self.events.dispatch(event);
        }

        // 6. Surface safety alerts from their durable facts (never from the
        //    in-memory outcomes) — a crash anywhere above cannot lose one; the
        //    next surfacing pass re-dispatches and consumers dedup by nonce.
        if let Err(error) = self.surface_pending_safety_alerts() {
            tracing::warn!(?error, "sync.receive_phase: surfacing safety alerts failed");
        }

        Ok((
            received + device_applied,
            fetched,
            rejected,
            unresolved,
            reject_reasons,
        ))
    }

    // `register_tokens` moved to `device_sync_loop.rs` (it registers the
    // same self-token the device-sync receive partition keys on).

    // =====================================================================
    // Send phase
    // =====================================================================

    /// Send phase: delegate to `SendPhase` for outgoing updates + ACKs.
    ///
    /// Moves the adapter into a `RelayClient` which is wrapped by a
    /// `SendPhase`. Pending payloads are pre-encrypted at queue time
    /// (propagation/features), so no ratchet state is loaded here.
    #[tracing::instrument(level = "debug", skip_all, name = "sync.send_phase")]
    fn run_send_phase(
        &self,
        identity: &crate::identity::Identity,
        contacts: &[Contact],
        adapter: HttpTransportAdapter,
    ) -> VauchiResult<crate::api::send_phase::SyncResult> {
        let our_id = hex::encode(identity.signing_public_key());

        // Build RelayClient wrapping the adapter
        let relay_config = self.config.relay.to_relay_client_config(
            self.config.delivery_receipts_enabled,
            self.config.suppress_presence,
        );
        let relay = RelayClient::new(adapter, relay_config, our_id).with_rng(self.rng.clone());

        let mut ctrl = SendPhase::new(
            relay,
            &self.storage,
            self.config.sync.clone(),
            self.events.clone(),
        )
        .with_local_device_id(*identity.device_id());

        // Connect the relay (adapter is already connected from receive phase)
        // SendPhase.connect() calls relay.connect() which calls
        // adapter.connect() — but the adapter is already connected, so the
        // health check runs again. This is fine for correctness.
        ctrl.connect(self.rng.as_ref())?;

        // Register mailbox tokens on the relay client for outbound routing
        let contact_keys: Vec<[u8; 32]> = contacts
            .iter()
            .filter_map(|c| c.shared_key().map(|k| *k.as_bytes()))
            .collect();
        // best-effort: token registration is idempotent and retried on
        // every sync cycle; a failure here means this cycle's send phase
        // routes against the previously-registered token set
        #[allow(clippy::let_underscore_must_use)]
        let _ = ctrl.relay_mut().register_mailbox_tokens_with_device_sync(
            &contact_keys,
            identity,
            0, // days_offline — Task 10 will compute this from last_connected_epoch
            self.clock.unix_seconds(),
            self.rng.as_ref(),
        );

        // Run the sync cycle (sends pending updates, processes ACKs)
        let result = ctrl.sync(self.rng.as_ref())?;

        // Flush queued device-sync items to linked devices over the same
        // connection (best-effort; never fails the contact-card cycle).
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.run_device_sync_send(&mut ctrl, identity);

        Ok(result)
    }

    // =====================================================================
    // Helpers
    // =====================================================================

    /// Update sync timing after a successful sync (C1 + C2).
    ///
    /// Computes the C2 deadline using a jittered sync interval. If the last
    /// exchange was recent (within `post_exchange_delay_max_ms`), a C1 deadline
    /// is also computed and the MAX of C1 and C2 is used. Also captures the
    /// wall-clock unix timestamp for `last_sync_time()` so MyInfoEngine can
    /// render a "Last synced X ago" caption (humble-UI follow-up to ios!472).
    fn update_timing_after_sync(&mut self) {
        let c2_deadline =
            self.monotonic.now() + self.config.sync.jittered_sync_interval(self.rng.as_ref());

        let deadline = if let Some(exchange_time) = self.last_exchange_time {
            let max_delay = Duration::from_millis(self.config.sync.post_exchange_delay_max_ms);
            if self.monotonic.now().duration_since(exchange_time) < max_delay {
                // Exchange was recent — enforce C1 as well
                let c1_deadline = exchange_time
                    + self
                        .config
                        .sync
                        .random_post_exchange_delay(self.rng.as_ref());
                c1_deadline.max(c2_deadline)
            } else {
                c2_deadline
            }
        } else {
            c2_deadline
        };

        self.next_sync_allowed = Some(deadline);
        self.last_sync_unix_seconds = Some(self.clock.unix_seconds());
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
        if let Some((cached_bytes, fetched_at)) =
            self.storage.ohttp_cache().load_ohttp_key(relay_url)?
            && self.is_ohttp_key_fresh(fetched_at)
        {
            if OhttpClient::new(cached_bytes.clone()).is_ok() {
                return Ok(cached_bytes);
            }
            self.storage.ohttp_cache().clear_ohttp_key(relay_url)?;
        }

        // 2. Fetch the live key when permitted: allow_direct (dev), or the
        //    OHTTP endpoint is a distinct IP-stripping relay (so the fetch
        //    doesn't leak IP to the data relay). Preferring the fetched key
        //    over the stale bundle survives gateway key rotation (problem
        //    2026-05-25-relay-ohttp-forward-hop-502); on failure/not-permitted
        //    fall through to the bundled offline bootstrap.
        let via_ohttp_relay = self.distinct_ohttp_route().is_some();
        if (self.config.ohttp.allow_direct || via_ohttp_relay)
            && let Ok(fetched) = self.fetch_and_cache_ohttp_key(relay_url)
        {
            return Ok(fetched);
        }

        // 3. Bundled key — offline/last-resort bootstrap (no network).
        if let Some(ref bundled) = self.config.ohttp.bundled_gateway_key {
            // Validate the bundled key before using it — skip if corrupt
            if OhttpClient::new(bundled.clone()).is_ok() {
                return Ok(bundled.clone());
            }
            // Invalid bundled key — fall through to error
        }

        Err(VauchiError::Network(
            crate::network::NetworkError::ConnectionFailed(
                "no OHTTP key available: cache expired, no bundled key, fetch failed/disabled"
                    .into(),
            ),
        ))
    }

    /// Check whether a cached OHTTP key is still within its TTL.
    fn is_ohttp_key_fresh(&self, fetched_at_epoch_secs: u64) -> bool {
        let now = self.clock.unix_seconds();
        let age = now.saturating_sub(fetched_at_epoch_secs);
        age < self.config.ohttp.key_ttl_secs
    }

    /// Fetch a fresh OHTTP key from the relay and cache it in storage.
    fn fetch_and_cache_ohttp_key(&self, relay_url: &str) -> VauchiResult<Vec<u8>> {
        let transport = self.create_bootstrap_transport_direct();
        let key_bytes = transport.fetch_ohttp_key().map_err(VauchiError::Network)?;
        OhttpClient::new(key_bytes.clone()).map_err(VauchiError::Network)?;
        self.storage
            .ohttp_cache()
            .save_ohttp_key(relay_url, &key_bytes)?;
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
    //
    // Vestigial since inception: this rotation entry point's only production
    // caller (`create_ohttp_adapter`) discarded its result — the sync path's
    // `relay_url` is always the distinct OHTTP host, so `ohttp_endpoint_pins`
    // returns `ohttp_pinned_certs` and ignores these rotated pins. Step 4's fold
    // removed that caller; the machinery is retained (still exercised by inline
    // tests) pending a wire-vs-retire decision — see
    // `backlog/2026-07-21-vestigial-pin-rotation` and the 2026-05-25
    // relay-ohttp-forward-hop record's "all three OHTTP sites" intent.
    #[allow(dead_code)]
    pub(crate) fn resolve_pins(&self) -> Vec<PinnedCertificate> {
        let mut pins = self.config.relay.pinned_certs.clone();

        // Pin rotation requires a verify key — without it, only bundled pins are used
        let Some(ref verify_key) = self.config.relay.pin_config_verify_key else {
            return pins;
        };

        let relay_url = self.http_relay_url();

        // Try loading cached pins (best-effort — cache miss is fine)
        if let Ok(Some((cached_pins, fetched_at))) =
            self.storage.pin_cache().load_pin_cache(&relay_url)
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
        let now = self.clock.unix_seconds();
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
        self.storage.pin_cache().save_pin_cache(relay_url, &pins)?;
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
        let relay_url = self.http_relay_url();
        // Bundled relay pins only (not resolve_pins) on the same-host path —
        // pin-config fetch must not depend on cached pins (circular dep).
        let pinned_certs =
            self.ohttp_endpoint_pins(&relay_url, self.config.relay.pinned_certs.clone());
        HttpTransport::new(HttpTransportConfig {
            relay_url,
            timeout_ms: self.config.relay.connect_timeout_ms,
            proxy: self.config.relay.proxy.clone(),
            allow_direct: true,
            pinned_certs,
        })
    }

    /// Pin set for a transport targeting `relay_url`.
    ///
    /// When `relay_url` is the distinct OHTTP-relay host (`ohttp.vauchi.app`
    /// in production — a separate entity with its own TLS key per ADR-037),
    /// pin ITS SPKI (`ohttp_pinned_certs`). Otherwise — the OHTTP endpoint
    /// shares the data-relay host (self-hosted / local / e2e), or the target
    /// is the data relay itself — use `relay_pins` (the data-relay set the
    /// caller would otherwise apply).
    ///
    /// Fixes 2026-05-25-relay-ohttp-forward-hop-502: pinning the relay's
    /// SPKI against `ohttp.vauchi.app` made every production sync fail
    /// `certificate pin verification failed`.
    fn ohttp_endpoint_pins(
        &self,
        relay_url: &str,
        relay_pins: Vec<PinnedCertificate>,
    ) -> Vec<PinnedCertificate> {
        let ohttp = self.http_relay_url();
        if relay_url == ohttp && ohttp != self.config.relay.server_url {
            self.config.relay.ohttp_pinned_certs.clone()
        } else {
            relay_pins
        }
    }

    /// Build a fail-closed `HttpTransport` for application relay actions.
    ///
    /// The outer endpoint is derived internally so callers cannot accidentally
    /// send OHTTP straight to the application relay. The application-relay URL
    /// parameter is retained for patch-compatible Rust consumers and checked
    /// against the configured application origin. Construction performs
    /// no network I/O: it uses the in-memory key, a fresh validated
    /// storage-cache entry, or the validated bundled key. Without one, action
    /// methods return their existing fail-closed error before sending a request.
    pub fn build_relay_transport(
        &self,
        application_relay_url: &str,
        timeout_ms: u64,
    ) -> HttpTransport {
        #[cfg(feature = "testing")]
        if self.config.ohttp.allow_direct {
            return HttpTransport::new(HttpTransportConfig::for_testing(
                self.config.relay.server_url.clone(),
                timeout_ms,
            ));
        }

        let route = self.action_ohttp_route(application_relay_url);
        let route_valid = route.is_some();
        let relay_url = route.unwrap_or_else(|| self.http_relay_url());
        let pinned_certs =
            self.ohttp_endpoint_pins(&relay_url, self.config.relay.pinned_certs.clone());
        let mut transport = HttpTransport::new(HttpTransportConfig {
            relay_url,
            timeout_ms,
            proxy: self.config.relay.proxy.clone(),
            allow_direct: false,
            pinned_certs,
        });

        if route_valid && let Some(client) = self.offline_ohttp_client() {
            transport.set_ohttp(client);
        }

        transport
    }

    /// Resolve an OHTTP client without performing network I/O.
    fn offline_ohttp_client(&self) -> Option<OhttpClient> {
        if let Some(ref client) = self.ohttp_key
            && let Ok(copy) = OhttpClient::new(client.encoded_config().to_vec())
        {
            return Some(copy);
        }

        let endpoint = self.http_relay_url();
        if let Ok(Some((bytes, fetched_at))) = self.storage.ohttp_cache().load_ohttp_key(&endpoint)
            && self.is_ohttp_key_fresh(fetched_at)
            && let Ok(client) = OhttpClient::new(bytes)
        {
            return Some(client);
        }

        self.config
            .ohttp
            .bundled_gateway_key
            .clone()
            .and_then(|bytes| OhttpClient::new(bytes).ok())
    }

    /// OHTTP-relay base URL for `/v2/ohttp` + the `/v2/ohttp-key` bootstrap,
    /// derived by [`crate::api::config::ohttp_endpoint`] (problem
    /// 2026-05-25-relay-ohttp-forward-hop-502).
    pub(crate) fn http_relay_url(&self) -> String {
        crate::api::config::ohttp_endpoint(
            &self.config.relay.server_url,
            self.config.relay.ohttp_relay_url.as_deref(),
        )
    }

    fn distinct_ohttp_route(&self) -> Option<String> {
        let application_origin = http_origin(&self.config.relay.server_url)?;
        let outer = self.http_relay_url();
        let outer_origin = http_origin(&outer)?;
        (application_origin != outer_origin).then_some(outer)
    }

    fn action_ohttp_route(&self, application_relay_url: &str) -> Option<String> {
        let configured_origin = http_origin(&self.config.relay.server_url)?;
        let requested_origin = http_origin(application_relay_url)?;
        (configured_origin == requested_origin)
            .then(|| self.distinct_ohttp_route())
            .flatten()
    }
}

/// One fetched relay blob, classified exactly once at the fetch site.
///
/// Step 3 of the consolidation plan: the receive loop used to re-triage
/// inside the `EncryptedUpdate` bucket with a token-set membership check
/// and a magic-prefix probe spread over two passes; this enum is the one
/// discriminator, and the downstream loops only match variants.
enum InboundBlob {
    /// Sealed for the shared identity (self-token): device sync.
    DeviceSync {
        message_id: String,
        ciphertext: Vec<u8>,
    },
    /// Magic-prefixed signed identity revocation (not encrypted).
    Revocation {
        message_id: String,
        revocation: crate::network::IdentityRevoked,
    },
    /// Ratchet-encrypted contact update, routed by mailbox token.
    ContactUpdate {
        message_id: String,
        token: String,
        ciphertext: Vec<u8>,
    },
}

/// Classification order is load-bearing: self-tokens first (device-sync
/// blobs are not decryptable by the contact router), then the revocation
/// magic prefix, then everything else is a contact update.
fn classify_inbound_blob(
    self_tokens: &std::collections::HashSet<String>,
    message_id: String,
    token: String,
    ciphertext: Vec<u8>,
) -> InboundBlob {
    if self_tokens.contains(&token) {
        return InboundBlob::DeviceSync {
            message_id,
            ciphertext,
        };
    }
    match crate::network::revocation::decode_revocation_blob(&ciphertext) {
        Some(revocation) => InboundBlob::Revocation {
            message_id,
            revocation,
        },
        None => InboundBlob::ContactUpdate {
            message_id,
            token,
            ciphertext,
        },
    }
}

fn http_origin(url: &str) -> Option<Origin> {
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.query().is_some()
    {
        return None;
    }
    Some(parsed.origin())
}

fn ohttp_route_error() -> VauchiError {
    VauchiError::Network(crate::network::NetworkError::ConnectionFailed(
        "OHTTP outer relay must use a distinct valid origin".into(),
    ))
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

    // @scenario: sync_privacy :: sync adapter never inherits direct fallback
    //
    // Step 4: the sync receive adapter is built through the hardened
    // `build_relay_transport` (the single OHTTP transport constructor).
    // With OHTTP unavailable and production `allow_direct = false`, it must
    // fail closed rather than dial the application relay directly.
    #[test]
    fn sync_adapter_never_inherits_direct_fallback() {
        use crate::api::VauchiConfig;
        use crate::network::{
            MessageEnvelope, MessagePayload, NetworkError, PROTOCOL_VERSION, RegisterMailbox,
            Transport,
        };

        let dir = tempfile::tempdir().expect("tempdir must succeed");
        let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
            .with_relay_url("http://127.0.0.1:1")
            .with_ohttp_relay_url("http://127.0.0.1:2");
        assert!(
            !config.ohttp.allow_direct,
            "test premise: production default disables direct connections"
        );
        let vauchi = Vauchi::new(config).expect("Vauchi::new must succeed");

        let server_url = vauchi.config.relay.server_url.clone();
        let timeout = vauchi.config.relay.connect_timeout_ms;
        let mut adapter =
            HttpTransportAdapter::new(vauchi.build_relay_transport(&server_url, timeout));
        adapter.clear_ohttp();
        adapter
            .send(&MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "register".to_string().into(),
                timestamp: 0,
                payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                    tokens: vec!["t".repeat(64)],
                }),
            })
            .expect("mailbox registration is local-only");

        let error = adapter
            .receive()
            .expect_err("missing OHTTP must fail before direct networking");
        assert!(
            matches!(
                error,
                NetworkError::ConnectionFailed(ref message)
                    if message == "OHTTP not configured and direct connections are disabled"
            ),
            "unexpected fail-closed error: {error:?}"
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
        let now = v.clock().unix_seconds();
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
        let now = v.clock().unix_seconds();
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

    // @scenario: pinning :: OHTTP transport pins the OHTTP host, not the relay
    #[test]
    fn ohttp_endpoint_pins_uses_ohttp_host_pin_for_distinct_host() {
        // Production: server_url = relay.vauchi.app, so the OHTTP endpoint
        // derives to the DISTINCT ohttp.vauchi.app. The OHTTP transport must
        // pin the OHTTP host's key, not the relay pins passed in
        // (2026-05-25-relay-ohttp-forward-hop-502).
        let (v, _dir) = vauchi_with_server_url("https://relay.vauchi.app");
        let ohttp = v.http_relay_url();
        assert_ne!(
            ohttp, v.config.relay.server_url,
            "precondition: production OHTTP endpoint is a distinct host"
        );
        let relay_sentinel = vec![PinnedCertificate::new([0x11; 32])];
        let pins = v.ohttp_endpoint_pins(&ohttp, relay_sentinel.clone());
        assert_eq!(
            pins, v.config.relay.ohttp_pinned_certs,
            "distinct OHTTP host must use the OHTTP-host pin set"
        );
        assert_ne!(
            pins, relay_sentinel,
            "must NOT pin the data-relay certs against the OHTTP host"
        );
        assert!(
            !pins.is_empty(),
            "production OHTTP-host pins must not be empty"
        );
    }

    // @scenario: pinning :: OHTTP shares the relay host -> relay pins apply
    #[test]
    fn ohttp_endpoint_pins_uses_relay_pins_when_same_host() {
        // Self-host / local: no distinct OHTTP relay, so the OHTTP endpoint
        // equals server_url and the data-relay pins (passed in) apply.
        let (v, _dir) = vauchi_with_server_url("http://127.0.0.1:8081");
        let target = v.http_relay_url();
        assert_eq!(
            target, v.config.relay.server_url,
            "precondition: non-production OHTTP endpoint equals server_url"
        );
        let relay_sentinel = vec![PinnedCertificate::new([0x22; 32])];
        let pins = v.ohttp_endpoint_pins(&target, relay_sentinel.clone());
        assert_eq!(
            pins, relay_sentinel,
            "same-host OHTTP endpoint must use the data-relay pins"
        );
    }

    // =========================================================================
    // last_sync_unix_seconds (humble-UI follow-up to ios!472)
    // =========================================================================

    // @scenario: ohttp_sync :: last_sync_time records wall-clock on success
    #[test]
    fn last_sync_time_records_wall_clock_via_update_timing() {
        use crate::clock::FakeClock;
        use std::time::{Duration, SystemTime};

        let pinned = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let clock = FakeClock::new(pinned).shared();
        let mut v = Vauchi::in_memory_with_clock(clock).unwrap();

        assert_eq!(
            v.last_sync_time(),
            None,
            "Vauchi starts with no recorded sync — last_sync_time must be None"
        );

        v.update_timing_after_sync();

        assert_eq!(
            v.last_sync_time(),
            Some(1_700_000_000),
            "update_timing_after_sync must capture clock().unix_seconds() so MyInfoEngine can render the relative caption"
        );
    }
}
