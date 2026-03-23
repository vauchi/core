// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Controller
//!
//! Orchestrates synchronization and network operations.

use std::collections::HashMap;
use std::sync::Arc;

use crate::crypto::ratchet::DoubleRatchetState;
use crate::network::delivery::{
    DeliveryAckStatus, DeliveryService, OfflineManager, RetryScheduler,
};
use crate::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use crate::network::{ConnectionState, RelayClient, Transport};
use crate::storage::Storage;
use crate::sync::device_sync::SyncItem;
use crate::sync::{DeviceSyncOrchestrator, SyncManager, SyncState};

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

/// Controls synchronization and network operations.
///
/// The SyncController orchestrates:
/// - Connection management
/// - Processing pending updates
/// - Handling acknowledgments
/// - Retry logic for failed updates
pub struct SyncController<'a, T: Transport> {
    relay: RelayClient<T>,
    sync_manager: SyncManager<'a>,
    delivery_service: DeliveryService,
    retry_scheduler: RetryScheduler,
    offline_manager: OfflineManager,
    storage: &'a Storage,
    config: SyncConfig,
    events: Arc<EventDispatcher>,
    /// Ratchet states per contact for encryption
    ratchets: HashMap<String, DoubleRatchetState>,
    /// Connection state tracking
    last_connection_state: ConnectionState,
}

impl<'a, T: Transport> SyncController<'a, T> {
    /// Creates a new SyncController.
    pub fn new(
        relay: RelayClient<T>,
        storage: &'a Storage,
        config: SyncConfig,
        events: Arc<EventDispatcher>,
    ) -> Self {
        SyncController {
            relay,
            sync_manager: SyncManager::new(storage),
            delivery_service: DeliveryService::new(),
            retry_scheduler: RetryScheduler::new(),
            offline_manager: OfflineManager::new(),
            storage,
            config,
            events,
            ratchets: HashMap::new(),
            last_connection_state: ConnectionState::Disconnected,
        }
    }

    /// Connects to the relay server.
    pub fn connect(&mut self) -> VauchiResult<()> {
        self.relay.connect()?;
        self.update_connection_state();
        Ok(())
    }

    /// Disconnects from the relay server.
    pub fn disconnect(&mut self) -> VauchiResult<()> {
        self.relay.disconnect()?;
        self.update_connection_state();
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

    /// Registers a ratchet state for a contact.
    ///
    /// The ratchet is used for end-to-end encryption of updates to this contact.
    pub fn register_ratchet(&mut self, contact_id: &str, ratchet: DoubleRatchetState) {
        self.ratchets.insert(contact_id.to_string(), ratchet);
    }

    /// Removes a ratchet state for a contact.
    pub fn remove_ratchet(&mut self, contact_id: &str) -> Option<DoubleRatchetState> {
        self.ratchets.remove(contact_id)
    }

    /// Checks if a ratchet exists for a contact.
    pub fn has_ratchet(&self, contact_id: &str) -> bool {
        self.ratchets.contains_key(contact_id)
    }

    /// Runs a sync cycle.
    ///
    /// This processes pending updates, sends them through the relay,
    /// and handles acknowledgments.
    pub fn sync(&mut self) -> VauchiResult<SyncResult> {
        if !self.is_connected() {
            return Err(VauchiError::Network(
                crate::network::NetworkError::NotConnected,
            ));
        }

        let mut result = SyncResult::default();

        // Process incoming messages (acknowledgments)
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

                // Route ACK events to delivery service for status tracking
                for event in &incoming.ack_events {
                    let ack_status =
                        DeliveryAckStatus::from_network_ack(event.status, event.error.as_deref());
                    // Best-effort delivery tracking — don't fail the sync cycle
                    // if a delivery record doesn't exist yet
                    let _ = self.delivery_service.handle_ack(
                        self.storage,
                        &event.update_id,
                        ack_status,
                    );
                }
            }
            Err(e) => {
                result.errors.push(("incoming".into(), e.to_string()));
            }
        }

        // Check for timed out messages
        let timed_out = self.relay.check_timeouts();
        for update_id in &timed_out {
            if let Some(update) = self.find_update_by_id(update_id) {
                let _ = self
                    .sync_manager
                    .mark_failed(update_id, "Timeout", update.retry_count + 1);
            }
            result.timed_out += 1;
        }

        // Get updates ready to send (pending or ready for retry)
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

        // Send each ready update
        let total = ready_updates.len();
        for (idx, update) in ready_updates.into_iter().enumerate() {
            // Skip if no ratchet for this contact
            let ratchet = match self.ratchets.get_mut(&update.contact_id) {
                Some(r) => r,
                None => {
                    // No ratchet available - skip this update
                    continue;
                }
            };

            // Load contact's shared key for anonymous sender ID and mailbox token
            let shared_key = self
                .storage
                .load_contact(&update.contact_id)
                .ok()
                .flatten()
                .map(|c| *c.shared_key().as_bytes());

            // Compute mailbox token as recipient_id (SP-33 Task 4.1)
            let recipient_id = match &shared_key {
                Some(key) => {
                    let token = compute_mailbox_token(key, current_day_epoch());
                    token_hex(&token)
                }
                None => update.contact_id.clone(),
            };

            // Send the update (anonymous sender ID if shared key available)
            match self.relay.send_update(
                &recipient_id,
                ratchet,
                &update.payload,
                &update.id,
                shared_key.as_ref(),
            ) {
                Ok(msg_id) => {
                    result.sent += 1;
                    self.events.dispatch(VauchiEvent::MessageDelivered {
                        contact_id: update.contact_id.clone(),
                        message_id: msg_id,
                    });
                }
                Err(e) => {
                    result.failed += 1;
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

        // Get ratchet for this contact
        let ratchet = match self.ratchets.get_mut(contact_id) {
            Some(r) => r,
            None => {
                return Err(VauchiError::InvalidState(format!(
                    "No ratchet for contact {}",
                    contact_id
                )));
            }
        };

        // Load contact's shared key for anonymous sender ID and mailbox token
        let shared_key = self
            .storage
            .load_contact(contact_id)
            .ok()
            .flatten()
            .map(|c| *c.shared_key().as_bytes());

        // Compute mailbox token as recipient_id (SP-33 Task 4.1)
        let recipient_id = match &shared_key {
            Some(key) => {
                let token = compute_mailbox_token(key, current_day_epoch());
                token_hex(&token)
            }
            None => contact_id.to_string(),
        };

        // Get pending updates for this contact
        let updates = self.sync_manager.get_pending(contact_id)?;

        for update in updates {
            match self.relay.send_update(
                &recipient_id,
                ratchet,
                &update.payload,
                &update.id,
                shared_key.as_ref(),
            ) {
                Ok(_) => {
                    result.sent += 1;
                }
                Err(e) => {
                    result.failed += 1;
                    result.errors.push((contact_id.to_string(), e.to_string()));
                }
            }
        }

        Ok(result)
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
    fn update_connection_state(&mut self) {
        let new_state = self.relay.connection().state();
        if new_state != self.last_connection_state {
            let was_disconnected =
                !matches!(self.last_connection_state, ConnectionState::Connected);
            self.last_connection_state = new_state.clone();
            self.events.dispatch(VauchiEvent::ConnectionStateChanged {
                state: new_state.clone(),
            });

            // On transition to Connected: flush offline queue and process retries
            if was_disconnected && new_state == ConnectionState::Connected {
                self.on_connectivity_restored();
            }
        }
    }

    /// Handles connectivity restoration — flushes offline queue and processes retries.
    ///
    /// Best-effort: errors are logged via events but don't propagate.
    fn on_connectivity_restored(&mut self) {
        // Process due retries
        if let Ok(tick_result) = self.retry_scheduler.tick(self.storage)
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

        // Flush offline queue — returns updates ready for sending
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

    // ============================================================
    // Device Sync Integration (Phase 7)
    // ============================================================

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

        // Serialize + encrypt for target device
        let payload_bytes = serde_json::to_vec(&sync_msg.items)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let ciphertext = orchestrator
            .encrypt_for_device(target_public_key, &payload_bytes)
            .map_err(VauchiError::DeviceSync)?;

        // We need a ratchet for device sync — if not available, skip
        let device_id_hex = hex::encode(target_device_id);
        let ratchet = self.ratchets.get_mut(&device_id_hex).ok_or_else(|| {
            VauchiError::InvalidState(format!("No ratchet for device {}", device_id_hex))
        })?;

        let ratchet_msg = ratchet
            .encrypt(&ciphertext)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        self.relay
            .send_device_sync_message(master_seed, ciphertext, &ratchet_msg)?;

        Ok(())
    }

    /// Processes incoming device sync items.
    ///
    /// Applies last-write-wins conflict resolution via the orchestrator.
    pub fn process_device_sync(
        &self,
        orchestrator: &mut DeviceSyncOrchestrator<'_>,
        incoming: Vec<SyncItem>,
    ) -> VauchiResult<Vec<SyncItem>> {
        let applied = orchestrator
            .process_incoming(incoming)
            .map_err(VauchiError::DeviceSync)?;

        Ok(applied)
    }
}
