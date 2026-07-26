// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Send phase of `Vauchi::sync()`.
//!
//! Drains pending, pre-encrypted updates through the relay, processes
//! ACKs and timeouts, and flushes device-sync batches over the same
//! connection. Constructed per cycle by `run_send_phase`
//! (`api/vauchi/sync_http.rs`) — deliberately NOT an orchestrator:
//! delta building, visibility filtering, encryption, and ratchet
//! persistence live upstream in `propagation.rs`/`features.rs` (send)
//! and `api/sync/card_update.rs` (receive). ADR-017 amendment
//! 2026-07-20 records the retirement of the orchestration claim.

use std::collections::HashMap;
use std::sync::Arc;

use crate::api::sync::{DeviceSyncOrchestrator, SyncManager};
use crate::network::delivery::{
    DeliveryAckStatus, DeliveryService, OfflineManager, RetryScheduler,
};
use crate::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use crate::network::{ConnectionState, RelayClient, Transport};
use crate::storage::Storage;
use crate::sync::SyncState;
use crate::sync::device_sync::SyncItem;

use super::config::SyncConfig;
use super::error::{VauchiError, VauchiResult};
use super::events::{EventDispatcher, VauchiEvent};

/// Result of a sync cycle.
#[derive(Debug, Default)]
pub struct SyncResult {
    /// Number of updates sent.
    pub sent: usize,
    /// Number of updates acknowledged.
    pub acknowledged: usize,
    /// Number of updates that failed.
    pub failed: usize,
    /// Number of timed out updates.
    pub timed_out: usize,
    /// Errors encountered.
    pub errors: Vec<(String, String)>,
}

impl SyncResult {
    /// Total number of operations processed (sent + acknowledged + failed + timed_out).
    pub fn total(&self) -> usize {
        self.sent + self.acknowledged + self.failed + self.timed_out
    }

    /// Whether any changes were synced (sent > 0 or acknowledged > 0).
    pub fn has_changes(&self) -> bool {
        self.sent > 0 || self.acknowledged > 0
    }
}

/// Per-cycle send-phase worker: relay connection lifecycle, draining
/// pre-built pending updates, ACK/timeout bookkeeping, and retry/offline
/// scheduling on reconnect.
pub struct SendPhase<'a, T: Transport> {
    relay: RelayClient<T>,
    sync_manager: SyncManager<'a>,
    delivery_service: DeliveryService,
    retry_scheduler: RetryScheduler,
    offline_manager: OfflineManager,
    storage: &'a Storage,
    config: SyncConfig,
    events: Arc<EventDispatcher>,
    /// Connection state tracking
    last_connection_state: ConnectionState,
}

impl<'a, T: Transport> SendPhase<'a, T> {
    /// Creates a new send-phase worker.
    pub fn new(
        relay: RelayClient<T>,
        storage: &'a Storage,
        config: SyncConfig,
        events: Arc<EventDispatcher>,
    ) -> Self {
        SendPhase {
            relay,
            sync_manager: SyncManager::new(storage),
            delivery_service: DeliveryService::new(),
            retry_scheduler: RetryScheduler::new(),
            offline_manager: OfflineManager::new(),
            storage,
            config,
            events,
            last_connection_state: ConnectionState::Disconnected,
        }
    }

    /// Connects to the relay server.
    pub fn connect(&mut self, rng: &dyn crate::rng::SecureRng) -> VauchiResult<()> {
        self.relay.connect()?;
        self.update_connection_state(rng);
        Ok(())
    }

    /// Disconnects from the relay server.
    pub fn disconnect(&mut self, rng: &dyn crate::rng::SecureRng) -> VauchiResult<()> {
        self.relay.disconnect()?;
        self.update_connection_state(rng);
        Ok(())
    }

    /// Returns true if connected to the relay.
    pub fn is_connected(&self) -> bool {
        self.relay.is_connected()
    }

    /// Returns the current connection state.
    pub fn connection_state(&self) -> ConnectionState {
        self.relay.connection().state()
    }

    /// Runs a sync cycle.
    ///
    /// This processes pending updates, sends them through the relay,
    /// and handles acknowledgments.
    pub fn sync(&mut self, rng: &dyn crate::rng::SecureRng) -> VauchiResult<SyncResult> {
        if !self.is_connected() {
            return Err(VauchiError::Network(
                crate::network::NetworkError::NotConnected,
            ));
        }

        tracing::info!("[Sync] started");
        let mut result = SyncResult::default();

        match self.relay.process_incoming() {
            Ok(incoming) => {
                for update_id in incoming.acknowledged {
                    match self.sync_manager.mark_delivered(&update_id) {
                        Err(e) => {
                            result.errors.push((update_id.clone(), e.to_string()));
                        }
                        _ => {
                            result.acknowledged += 1;
                        }
                    }
                }

                for event in &incoming.ack_events {
                    let ack_status =
                        DeliveryAckStatus::from_network_ack(event.status, event.error.as_deref());
                    // Best-effort delivery tracking — don't fail the sync cycle
                    // if a delivery record doesn't exist yet
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = self.delivery_service.handle_ack(
                        self.storage,
                        &event.update_id,
                        ack_status,
                        rng,
                    );
                }
            }
            Err(e) => {
                result.errors.push(("incoming".into(), e.to_string()));
            }
        }

        let timed_out = self.relay.check_timeouts();
        for update_id in &timed_out {
            if let Some(update) = self.find_update_by_id(update_id) {
                // best-effort: timeout marking is advisory; sync_manager
                // will re-discover the update on the next cycle if this fails
                #[allow(clippy::let_underscore_must_use)]
                let _ = self
                    .sync_manager
                    .mark_failed(update_id, "Timeout", update.retry_count + 1);
            }
            result.timed_out += 1;
        }

        let mut ready_updates = match self.sync_manager.get_ready_for_retry() {
            Ok(updates) => updates,
            Err(e) => {
                result.errors.push(("get_ready".into(), e.to_string()));
                return Ok(result);
            }
        };

        // Apply batch_size limit (#64) — cap updates per cycle to avoid
        // blocking the thread when a large backlog exists.
        if let Some(batch_size) = self.config.batch_size
            && batch_size > 0
        {
            ready_updates.truncate(batch_size);
        }

        let total = ready_updates.len();
        for (idx, update) in ready_updates.into_iter().enumerate() {
            // The pending payload is already encrypted and its matching
            // per-device ratchet was persisted atomically before queueing —
            // no ratchet state is needed (or held) here.

            // Load contact's shared key for anonymous sender ID and mailbox token.
            // ADR-029 forbids stable (non-rotating) recipient_ids: if we can't
            // derive a daily-rotating mailbox token for this contact, the
            // update MUST be skipped. Pre-2026-05-23 this path fell back to
            // `update.contact_id.clone()` — a stable plaintext id — under any
            // storage fault, missing contact, or incomplete exchange.
            let (shared_key, recipient_pk) =
                match self.storage.contacts().load_contact(&update.contact_id) {
                    Ok(Some(c)) => (
                        c.shared_key().map(|k| *k.as_bytes()),
                        c.public_key().copied(),
                    ),
                    Ok(None) => {
                        result.failed += 1;
                        result.errors.push((
                            update.contact_id.clone(),
                            "contact not found in storage; cannot derive mailbox token \
                             (ADR-029 forbids stable plaintext recipient_id)"
                                .into(),
                        ));
                        continue;
                    }
                    Err(e) => {
                        result.failed += 1;
                        result
                            .errors
                            .push((update.contact_id.clone(), e.to_string()));
                        continue;
                    }
                };
            let (Some(key), Some(recipient_pk)) = (shared_key, recipient_pk) else {
                result.failed += 1;
                result.errors.push((
                    update.contact_id.clone(),
                    "contact has no shared_key/public_key; cannot derive mailbox token \
                     (ADR-029 forbids stable plaintext recipient_id)"
                        .into(),
                ));
                continue;
            };
            let shared_key = Some(key);

            // Compute the recipient's directional mailbox token (keyed to the
            // contact's identity) — SP-33 Task 4.1 + directional tokens 2026-06-30.
            // A per-device fan-out copy deposits at the recipient DEVICE's
            // device-scoped mailbox so no sibling can drain it (F4, ADR-064
            // Amendment 2026-07-25); legacy/genesis copies keep the identity
            // mailbox.
            let day = current_day_epoch(self.storage.clock().unix_seconds());
            let token = match update.target_device_id {
                Some(device_id) => crate::network::mailbox_token::compute_device_mailbox_token(
                    &key,
                    &recipient_pk,
                    &device_id,
                    day,
                ),
                None => compute_mailbox_token(&key, &recipient_pk, day),
            };
            let recipient_id = token_hex(&token);

            // Send the PRE-BUILT message as-is — no re-encryption.
            let ratchet_msg: crate::crypto::ratchet::RatchetMessage =
                match serde_json::from_slice(&update.payload) {
                    Ok(msg) => msg,
                    Err(e) => {
                        result.failed += 1;
                        result
                            .errors
                            .push((update.contact_id.clone(), format!("payload decode: {e}")));
                        continue;
                    }
                };
            let sender_device_id = match self.sender_device_id_for(&update) {
                Ok(device_id) => device_id,
                Err(error) => {
                    result.failed += 1;
                    result
                        .errors
                        .push((update.contact_id.clone(), error.to_string()));
                    continue;
                }
            };
            match self.relay.send_raw_update_for_device(
                self.storage.clock().unix_seconds(),
                &recipient_id,
                &ratchet_msg,
                &update.id,
                shared_key.as_ref(),
                sender_device_id.as_ref(),
            ) {
                Ok(msg_id) => {
                    result.sent += 1;
                    // The relay accepted (stored) the blob; store-and-forward
                    // guarantees the recipient fetches it (now, or via catch-up
                    // token registration). Clear the pending update so it is NOT
                    // re-sent every sync — re-sending the SAME ratchet message
                    // makes the receiver decrypt-fail it (its ratchet advanced
                    // past that message), churning the receive path and burying
                    // real updates. The prior ack-based clear never fired because
                    // `run_send_phase` rebuilds the `RelayClient` each cycle, so
                    // the in-memory in-flight map (and thus the delivery ack) was
                    // lost before the next poll (2026-06-30). Delivery receipts
                    // are tracked separately via `delivery_service`.
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = self.storage.pending().mark_update_sent(&update.id);
                    self.events.dispatch(VauchiEvent::MessageDelivered {
                        contact_id: update.contact_id.clone(),
                        message_id: msg_id.into_string(),
                    });
                }
                Err(e) => {
                    result.failed += 1;
                    // best-effort: error already recorded in result.errors below;
                    // sync_manager mark_failed is advisory for retry scheduling
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = self.sync_manager.mark_failed(
                        &update.id,
                        &e.to_string(),
                        update.retry_count + 1,
                    );
                    result
                        .errors
                        .push((update.contact_id.clone(), e.to_string()));
                    self.events.dispatch(VauchiEvent::MessageFailed {
                        contact_id: update.contact_id.clone(),
                        error: e.to_string(),
                    });
                }
            }

            self.events.dispatch(VauchiEvent::SyncProgress {
                total,
                processed: idx + 1,
                contact_id: update.contact_id,
            });
        }

        tracing::info!(
            "[Sync] complete: {} sent, {} acknowledged",
            result.sent,
            result.acknowledged
        );
        Ok(result)
    }

    /// Syncs updates for a specific contact only.
    pub fn sync_contact(&mut self, contact_id: &str) -> VauchiResult<SyncResult> {
        if !self.is_connected() {
            return Err(VauchiError::Network(
                crate::network::NetworkError::NotConnected,
            ));
        }

        let mut result = SyncResult::default();

        // Pre-built pending payloads already prove encryption succeeded. Their
        // sessions may exist only in the per-device store, not this legacy map.

        // Load contact's shared key for anonymous sender ID and mailbox token.
        // ADR-029 forbids stable (non-rotating) recipient_ids: if we can't
        // derive a daily-rotating mailbox token for this contact, return
        // a typed error rather than fall back to `contact_id.to_string()`
        // (a stable plaintext id). Pre-2026-05-23 this path also produced
        // the ADR-029 violation.
        let (shared_key, recipient_pk) = match self.storage.contacts().load_contact(contact_id)? {
            Some(c) => (
                c.shared_key().map(|k| *k.as_bytes()),
                c.public_key().copied(),
            ),
            None => {
                return Err(VauchiError::ContactNotFound(contact_id.to_string()));
            }
        };
        let (Some(key), Some(recipient_pk)) = (shared_key, recipient_pk) else {
            return Err(VauchiError::InvalidState(format!(
                "contact {contact_id} has no shared_key/public_key; cannot derive mailbox token \
                 (ADR-029 forbids stable plaintext recipient_id)"
            )));
        };
        let shared_key = Some(key);

        let day = current_day_epoch(self.storage.clock().unix_seconds());

        let updates = self.sync_manager.get_pending(contact_id)?;

        for update in updates {
            // Per-update mailbox token: a per-device fan-out copy deposits at
            // the recipient DEVICE's device-scoped mailbox (F4, ADR-064
            // Amendment 2026-07-25); legacy/genesis copies keep the identity
            // mailbox (SP-33 Task 4.1 + directional tokens 2026-06-30).
            let token = match update.target_device_id {
                Some(device_id) => crate::network::mailbox_token::compute_device_mailbox_token(
                    &key,
                    &recipient_pk,
                    &device_id,
                    day,
                ),
                None => compute_mailbox_token(&key, &recipient_pk, day),
            };
            let recipient_id = token_hex(&token);

            // Send the PRE-BUILT message as-is — no re-encryption.
            let ratchet_msg: crate::crypto::ratchet::RatchetMessage =
                match serde_json::from_slice(&update.payload) {
                    Ok(msg) => msg,
                    Err(e) => {
                        result.failed += 1;
                        result
                            .errors
                            .push((contact_id.to_string(), format!("payload decode: {e}")));
                        continue;
                    }
                };
            let sender_device_id = self.sender_device_id_for(&update)?;
            match self.relay.send_raw_update_for_device(
                self.storage.clock().unix_seconds(),
                &recipient_id,
                &ratchet_msg,
                &update.id,
                shared_key.as_ref(),
                sender_device_id.as_ref(),
            ) {
                Ok(_) => {
                    result.sent += 1;
                    // Clear on relay-accept so the update is not re-sent every
                    // sync (see the matching note in `sync`'s send loop).
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = self.storage.pending().mark_update_sent(&update.id);
                }
                Err(e) => {
                    result.failed += 1;
                    result.errors.push((contact_id.to_string(), e.to_string()));
                }
            }
        }

        Ok(result)
    }

    /// Choose the envelope sender token for a pending update.
    ///
    /// A genesis pre-`Active` handshake message, or any identity-mailbox send
    /// (`target_device_id: None`), MUST use the legacy token: the peer may not
    /// yet know this device and can resolve the contact only via the legacy
    /// token, then reads our real device id from the signed genesis envelope.
    /// Keying on the message — not on whether we happen to know the peer's
    /// devices — is the F4 lost-primary fix (2026-07-26 investigation). Only a
    /// device-scoped card delta to a peer that already knows us gets this
    /// device's scoped token.
    fn sender_device_id_for(
        &self,
        update: &crate::storage::PendingUpdate,
    ) -> VauchiResult<Option<[u8; 32]>> {
        if update.update_type == crate::api::sync::REGISTRY_HANDSHAKE_UPDATE_TYPE
            || update.target_device_id.is_none()
        {
            return Ok(None);
        }
        self.storage
            .device()
            .load_device_info()?
            .map(|info| Some(info.0))
            .ok_or_else(|| {
                VauchiError::InvalidState(
                    "local device info is required for a device-scoped sender token".into(),
                )
            })
    }

    /// Gets the sync state for a contact.
    pub fn get_sync_state(&self, contact_id: &str) -> VauchiResult<SyncState> {
        Ok(self.sync_manager.get_sync_state(contact_id)?)
    }

    /// Gets sync states for all contacts with pending updates.
    pub fn sync_status(&self) -> VauchiResult<HashMap<String, SyncState>> {
        Ok(self.sync_manager.sync_status()?)
    }

    /// Returns the number of pending updates across all contacts.
    pub fn pending_count(&self) -> VauchiResult<usize> {
        Ok(self.sync_manager.get_all_pending()?.len())
    }

    /// Returns the number of in-flight messages.
    pub fn in_flight_count(&self) -> usize {
        self.relay.in_flight_count()
    }

    /// Returns true if auto-sync is enabled.
    pub fn is_auto_sync_enabled(&self) -> bool {
        self.config.auto_sync
    }

    /// Returns a reference to the underlying relay client.
    pub fn relay(&self) -> &RelayClient<T> {
        &self.relay
    }

    /// Returns a mutable reference to the underlying relay client.
    pub fn relay_mut(&mut self) -> &mut RelayClient<T> {
        &mut self.relay
    }

    /// Returns a reference to the sync manager.
    pub fn sync_manager(&self) -> &SyncManager<'a> {
        &self.sync_manager
    }

    /// Returns a reference to the retry scheduler.
    pub fn retry_scheduler(&self) -> &RetryScheduler {
        &self.retry_scheduler
    }

    /// Returns a reference to the offline manager.
    pub fn offline_manager(&self) -> &OfflineManager {
        &self.offline_manager
    }

    /// Updates connection state and emits event if changed.
    ///
    /// When transitioning to `Connected`, triggers retry tick and
    /// offline queue flush to process pending work.
    fn update_connection_state(&mut self, rng: &dyn crate::rng::SecureRng) {
        let new_state = self.relay.connection().state();
        if new_state != self.last_connection_state {
            let was_disconnected =
                !matches!(self.last_connection_state, ConnectionState::Connected);
            self.last_connection_state = new_state.clone();
            self.events.dispatch(VauchiEvent::ConnectionStateChanged {
                state: new_state.clone(),
            });

            if was_disconnected && new_state == ConnectionState::Connected {
                self.on_connectivity_restored(rng);
            }
        }
    }

    /// Handles connectivity restoration — flushes offline queue and processes retries.
    ///
    /// Best-effort: errors are logged via events but don't propagate.
    fn on_connectivity_restored(&mut self, rng: &dyn crate::rng::SecureRng) {
        if let Ok(tick_result) = self.retry_scheduler.tick(self.storage, rng)
            && (tick_result.rescheduled > 0 || tick_result.expired > 0)
        {
            self.events.dispatch(VauchiEvent::DeliveryStatusUpdate {
                message_id: String::new(),
                status: format!(
                    "Retry tick: {} rescheduled, {} expired",
                    tick_result.rescheduled, tick_result.expired
                ),
            });
        }

        if let Ok(flushed) = self.offline_manager.flush_queue(self.storage)
            && !flushed.is_empty()
        {
            self.events.dispatch(VauchiEvent::DeliveryStatusUpdate {
                message_id: String::new(),
                status: format!("{} offline updates ready for send", flushed.len()),
            });
        }
    }

    /// Finds an update by its ID.
    fn find_update_by_id(&self, update_id: &str) -> Option<crate::storage::PendingUpdate> {
        self.sync_manager
            .get_all_pending()
            .ok()?
            .into_iter()
            .find(|u| u.id == update_id)
    }

    /// Sends pending device sync items to another device via self-token routing.
    ///
    /// Encrypts the sync payload and wraps it in an `EncryptedUpdate` where
    /// the `recipient_id` is the daily self-token derived from the master seed.
    /// All devices sharing the same master seed will receive this message.
    ///
    /// SP-33 Task 4.3.
    pub fn send_device_sync(
        &mut self,
        orchestrator: &DeviceSyncOrchestrator<'_>,
        target_device_id: &[u8; 32],
        target_public_key: &[u8; 32],
        master_seed: &[u8; 32],
    ) -> VauchiResult<()> {
        let sync_msg = orchestrator
            .create_sync_message(target_device_id)
            .map_err(VauchiError::DeviceSync)?;

        if sync_msg.items.is_empty() {
            return Ok(());
        }

        // Serialize + seal for the target device. `encrypt_for_device` is
        // ECDH (our device exchange key × the target device public key, both
        // derived from the shared master seed) + HKDF + XChaCha20-Poly1305 —
        // self-contained authenticated encryption with no interactive
        // handshake. No Double Ratchet: every same-seed device can already
        // derive this key, so a ratchet adds no confidentiality (see the
        // `2026-06-06-multi-device-sync-live-wiring` investigation §2).
        let payload_bytes = serde_json::to_vec(&sync_msg.items)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let ciphertext = orchestrator
            .encrypt_for_device(target_public_key, &payload_bytes)
            .map_err(VauchiError::DeviceSync)?;

        self.relay.send_device_sync_message(
            master_seed,
            target_device_id,
            ciphertext,
            self.storage.clock().unix_seconds(),
        )?;

        Ok(())
    }

    /// Processes incoming device sync items.
    ///
    /// Applies last-write-wins conflict resolution via the orchestrator.
    pub fn process_device_sync(
        &self,
        orchestrator: &mut DeviceSyncOrchestrator<'_>,
        incoming: Vec<SyncItem>,
        sender_device_id: &[u8; 32],
    ) -> VauchiResult<Vec<SyncItem>> {
        let applied = orchestrator
            .process_incoming(incoming, sender_device_id)
            .map_err(VauchiError::DeviceSync)?;

        Ok(applied)
    }
}
