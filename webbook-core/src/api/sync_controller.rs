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
use super::error::{WebBookError, WebBookResult};
use super::events::{EventDispatcher, WebBookEvent};

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
    pub fn connect(&mut self) -> WebBookResult<()> {
        self.relay.connect()?;
        self.update_connection_state();
        Ok(())
    }

    /// Disconnects from the relay server.
    pub fn disconnect(&mut self) -> WebBookResult<()> {
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
    pub fn sync(&mut self) -> WebBookResult<SyncResult> {
        if !self.is_connected() {
            return Err(WebBookError::Network(
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
                    self.events.dispatch(WebBookEvent::MessageDelivered {
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
                    self.events.dispatch(WebBookEvent::MessageFailed {
                        contact_id: update.contact_id,
                        error: e.to_string(),
                    });
                }
            }
        }

        Ok(result)
    }

    /// Syncs updates for a specific contact only.
    pub fn sync_contact(&mut self, contact_id: &str) -> WebBookResult<SyncResult> {
        if !self.is_connected() {
            return Err(WebBookError::Network(
                crate::network::NetworkError::NotConnected,
            ));
        }

        let mut result = SyncResult::default();

        // Get ratchet for this contact
        let ratchet = match self.ratchets.get_mut(contact_id) {
            Some(r) => r,
            None => {
                return Err(WebBookError::InvalidState(format!(
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
    pub fn get_sync_state(&self, contact_id: &str) -> WebBookResult<SyncState> {
        Ok(self.sync_manager.get_sync_state(contact_id)?)
    }

    /// Gets sync states for all contacts with pending updates.
    pub fn sync_status(&self) -> WebBookResult<HashMap<String, SyncState>> {
        Ok(self.sync_manager.sync_status()?)
    }

    /// Returns the number of pending updates across all contacts.
    pub fn pending_count(&self) -> WebBookResult<usize> {
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
                .dispatch(WebBookEvent::ConnectionStateChanged { state: new_state });
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
    ) -> WebBookResult<()> {
        // Get pending items for target device
        let pending = orchestrator.pending_for_device(target_device_id);
        if pending.is_empty() {
            return Ok(()); // Nothing to send
        }

        // Serialize pending items
        let payload = serde_json::to_vec(&pending).map_err(|e| {
            WebBookError::InvalidState(format!("Failed to serialize sync items: {}", e))
        })?;

        // Encrypt for target device
        let _ciphertext = orchestrator
            .encrypt_for_device(target_public_key, &payload)
            .map_err(WebBookError::DeviceSync)?;

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
    ) -> WebBookResult<Vec<SyncItem>> {
        let applied = orchestrator
            .process_incoming(incoming)
            .map_err(WebBookError::DeviceSync)?;

        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SymmetricKey;
    use crate::exchange::X3DHKeyPair;
    use crate::network::{MockTransport, RelayClientConfig, TransportConfig};

    fn create_test_storage() -> Storage {
        let key = SymmetricKey::generate();
        Storage::in_memory(key).unwrap()
    }

    fn create_test_relay() -> RelayClient<MockTransport> {
        let transport = MockTransport::new();
        let config = RelayClientConfig {
            transport: TransportConfig::default(),
            max_pending_messages: 100,
            ack_timeout_ms: 30_000,
            max_retries: 3,
        };
        RelayClient::new(transport, config, "test-identity".into())
    }

    fn create_test_ratchet() -> DoubleRatchetState {
        let bob_dh = X3DHKeyPair::generate();
        let shared_secret = SymmetricKey::generate();
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key())
    }

    #[test]
    fn test_sync_controller_connect_disconnect() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let mut controller = SyncController::new(relay, &storage, config, events);

        assert!(!controller.is_connected());

        controller.connect().unwrap();
        assert!(controller.is_connected());

        controller.disconnect().unwrap();
        assert!(!controller.is_connected());
    }

    #[test]
    fn test_sync_controller_ratchet_management() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let mut controller = SyncController::new(relay, &storage, config, events);

        let ratchet = create_test_ratchet();
        controller.register_ratchet("contact-1", ratchet);

        assert!(controller.has_ratchet("contact-1"));
        assert!(!controller.has_ratchet("contact-2"));

        let removed = controller.remove_ratchet("contact-1");
        assert!(removed.is_some());
        assert!(!controller.has_ratchet("contact-1"));
    }

    #[test]
    fn test_sync_controller_sync_not_connected() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let mut controller = SyncController::new(relay, &storage, config, events);

        // Should fail when not connected
        let result = controller.sync();
        assert!(matches!(result, Err(WebBookError::Network(_))));
    }

    #[test]
    fn test_sync_controller_sync_empty() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let mut controller = SyncController::new(relay, &storage, config, events);
        controller.connect().unwrap();

        // Sync with no pending updates
        let result = controller.sync().unwrap();
        assert_eq!(result.sent, 0);
        assert_eq!(result.acknowledged, 0);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_sync_controller_get_sync_state() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let controller = SyncController::new(relay, &storage, config, events);

        // No pending updates = synced
        let state = controller.get_sync_state("contact-1").unwrap();
        assert!(matches!(state, SyncState::Synced { .. }));
    }

    #[test]
    fn test_sync_controller_pending_count() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let controller = SyncController::new(relay, &storage, config, events);

        // Initially no pending
        assert_eq!(controller.pending_count().unwrap(), 0);
    }

    #[test]
    fn test_sync_controller_in_flight_count() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let controller = SyncController::new(relay, &storage, config, events);

        // Initially no in-flight
        assert_eq!(controller.in_flight_count(), 0);
    }

    #[test]
    fn test_sync_controller_auto_sync_config() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());

        // Test with auto_sync enabled
        let config = SyncConfig {
            auto_sync: true,
            ..Default::default()
        };
        let controller = SyncController::new(relay, &storage, config, events.clone());
        assert!(controller.is_auto_sync_enabled());

        // Test with auto_sync disabled
        let relay2 = create_test_relay();
        let config2 = SyncConfig {
            auto_sync: false,
            ..Default::default()
        };
        let controller2 = SyncController::new(relay2, &storage, config2, events);
        assert!(!controller2.is_auto_sync_enabled());
    }

    #[test]
    fn test_sync_result_default() {
        let result = SyncResult::default();
        assert_eq!(result.sent, 0);
        assert_eq!(result.acknowledged, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(result.timed_out, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_sync_controller_sync_contact_no_ratchet() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let mut controller = SyncController::new(relay, &storage, config, events);
        controller.connect().unwrap();

        // Should fail with no ratchet
        let result = controller.sync_contact("contact-1");
        assert!(matches!(result, Err(WebBookError::InvalidState(_))));
    }

    #[test]
    fn test_sync_controller_sync_contact_with_ratchet() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let mut controller = SyncController::new(relay, &storage, config, events);
        controller.connect().unwrap();

        // Register ratchet
        let ratchet = create_test_ratchet();
        controller.register_ratchet("contact-1", ratchet);

        // Should succeed (no pending updates)
        let result = controller.sync_contact("contact-1").unwrap();
        assert_eq!(result.sent, 0);
    }

    // ============================================================
    // Phase 7: Device Sync Integration Tests (TDD)
    // ============================================================

    use crate::crypto::SigningKeyPair;
    use crate::identity::device::{DeviceInfo, DeviceRegistry};
    use crate::sync::{DeviceSyncOrchestrator, SyncItem};

    fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
        DeviceInfo::derive(master_seed, index, name.to_string())
    }

    fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
        let signing_key = SigningKeyPair::from_seed(master_seed);
        DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
    }

    #[test]
    fn test_sync_controller_send_device_sync() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let mut controller = SyncController::new(relay, &storage, config, events);
        controller.connect().unwrap();

        // Create device orchestrator
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);
        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_id = *device_b.device_id();
        let device_b_public_key = *device_b.exchange_public_key();

        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Record a local change
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "test@example.com".to_string(),
                timestamp: 1000,
            })
            .unwrap();

        // Send device sync via controller
        let result = controller.send_device_sync(&orchestrator, &device_b_id, &device_b_public_key);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sync_controller_process_device_sync() {
        let storage = create_test_storage();
        let relay = create_test_relay();
        let events = Arc::new(EventDispatcher::new());
        let config = SyncConfig::default();

        let controller = SyncController::new(relay, &storage, config, events);

        // Create device orchestrator
        let master_seed = [0x42u8; 32];
        let device = create_test_device(&master_seed, 0, "Test Device");
        let registry = create_test_registry(&master_seed, &device);

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device, registry);

        // Create incoming sync items
        let incoming = vec![SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1234567890".to_string(),
            timestamp: 1000,
        }];

        // Process via controller
        let applied = controller.process_device_sync(&mut orchestrator, incoming);
        assert!(applied.is_ok());
        assert_eq!(applied.unwrap().len(), 1);
    }
}
