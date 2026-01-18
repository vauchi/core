//! Sync Controller
//!
//! Orchestrates synchronization and network operations.

use std::collections::HashMap;
use std::sync::Arc;

use crate::crypto::ratchet::DoubleRatchetState;
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
            Ok(acked_ids) => {
                for update_id in acked_ids {
                    if let Err(e) = self.sync_manager.mark_delivered(&update_id) {
                        result.errors.push((update_id.clone(), e.to_string()));
                    } else {
                        result.acknowledged += 1;
                    }
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
        let ready_updates = match self.sync_manager.get_ready_for_retry() {
            Ok(updates) => updates,
            Err(e) => {
                result.errors.push(("get_ready".into(), e.to_string()));
                return Ok(result);
            }
        };

        // Send each ready update
        for update in ready_updates {
            // Skip if no ratchet for this contact
            let ratchet = match self.ratchets.get_mut(&update.contact_id) {
                Some(r) => r,
                None => {
                    // No ratchet available - skip this update
                    continue;
                }
            };

            // Send the update
            match self
                .relay
                .send_update(&update.contact_id, ratchet, &update.payload, &update.id)
            {
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
                        contact_id: update.contact_id,
                        error: e.to_string(),
                    });
                }
            }
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

        // Get pending updates for this contact
        let updates = self.sync_manager.get_pending(contact_id)?;

        for update in updates {
            match self
                .relay
                .send_update(contact_id, ratchet, &update.payload, &update.id)
            {
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

    /// Updates connection state and emits event if changed.
    fn update_connection_state(&mut self) {
        let new_state = self.relay.connection().state();
        if new_state != self.last_connection_state {
            self.last_connection_state = new_state.clone();
            self.events
                .dispatch(VauchiEvent::ConnectionStateChanged { state: new_state });
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

    /// Sends pending device sync items to another device.
    ///
    /// Creates an encrypted sync message and sends it via the relay.
    pub fn send_device_sync(
        &self,
        orchestrator: &DeviceSyncOrchestrator<'_>,
        target_device_id: &[u8; 32],
        target_public_key: &[u8; 32],
    ) -> VauchiResult<()> {
        // Get pending items for target device
        let pending = orchestrator.pending_for_device(target_device_id);
        if pending.is_empty() {
            return Ok(()); // Nothing to send
        }

        // Serialize pending items
        let payload = serde_json::to_vec(&pending).map_err(|e| {
            VauchiError::InvalidState(format!("Failed to serialize sync items: {}", e))
        })?;

        // Encrypt for target device
        let _ciphertext = orchestrator
            .encrypt_for_device(target_public_key, &payload)
            .map_err(VauchiError::DeviceSync)?;

        // TODO: Send via relay when DeviceSyncMessage routing is implemented
        // For now, the encryption/preparation is what we're testing

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
