//! Device Sync Orchestrator
//!
//! Orchestrates synchronization between devices belonging to the same identity.
//! Manages queuing of SyncItems, tracking sync state per device, and version vectors.

use std::collections::HashMap;

use crate::contact_card::ContactCard;
use crate::crypto::{encryption, SymmetricKey, HKDF};
use crate::identity::device::{DeviceInfo, DeviceRegistry};
use crate::storage::Storage;
use crate::sync::device_sync::{
    DeviceSyncError, DeviceSyncPayload, InterDeviceSyncState, SyncItem, VersionVector,
};

/// Domain separation for device-to-device encryption key derivation.
const DEVICE_SYNC_INFO: &[u8] = b"WebBook_DeviceSync";

/// Orchestrates synchronization between devices of the same identity.
///
/// Each instance manages sync state for a single device, tracking what
/// needs to be synced to other linked devices.
pub struct DeviceSyncOrchestrator<'a> {
    /// Storage for persisting state.
    storage: &'a Storage,
    /// Current device info.
    current_device: DeviceInfo,
    /// Device registry (all linked devices).
    registry: DeviceRegistry,
    /// Per-device sync state (device_id -> sync state).
    device_states: HashMap<[u8; 32], InterDeviceSyncState>,
    /// Local version vector for causality tracking.
    version_vector: VersionVector,
    /// Timestamps of the last change to each field (for conflict resolution).
    /// Key is the field identifier (e.g., "field:email" or "contact:abc123").
    field_timestamps: HashMap<String, u64>,
}

impl<'a> DeviceSyncOrchestrator<'a> {
    /// Creates a new device sync orchestrator.
    pub fn new(storage: &'a Storage, current_device: DeviceInfo, registry: DeviceRegistry) -> Self {
        // Initialize sync states for all other devices
        let mut device_states = HashMap::new();
        for device in registry.active_devices() {
            if device.device_id != *current_device.device_id() {
                device_states.insert(
                    device.device_id,
                    InterDeviceSyncState::new(device.device_id),
                );
            }
        }

        DeviceSyncOrchestrator {
            storage,
            current_device,
            registry,
            device_states,
            version_vector: VersionVector::new(),
            field_timestamps: HashMap::new(),
        }
    }

    /// Creates an orchestrator and loads existing state from storage.
    pub fn load(
        storage: &'a Storage,
        current_device: DeviceInfo,
        registry: DeviceRegistry,
    ) -> Result<Self, DeviceSyncError> {
        let mut orchestrator = Self::new(storage, current_device, registry);

        // Load existing sync states from storage
        let stored_states = storage
            .list_device_sync_states()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        for state in stored_states {
            orchestrator.device_states.insert(*state.device_id(), state);
        }

        // Load version vector if exists
        if let Some(vector) = storage
            .load_version_vector()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
        {
            orchestrator.version_vector = vector;
        }

        Ok(orchestrator)
    }

    /// Records a local change to be synced to other devices.
    ///
    /// Queues the SyncItem for all other linked devices and increments
    /// the local version vector.
    pub fn record_local_change(&mut self, item: SyncItem) -> Result<(), DeviceSyncError> {
        // Track timestamp for conflict resolution
        let key = Self::conflict_key(&item);
        let timestamp = item.timestamp();
        self.field_timestamps.insert(key, timestamp);

        // Increment our version
        self.version_vector
            .increment(self.current_device.device_id());

        // Queue item for all other devices
        for state in self.device_states.values_mut() {
            state.queue_item(item.clone());
        }

        // Persist updated states
        for state in self.device_states.values() {
            self.storage
                .save_device_sync_state(state)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }

        // Persist version vector
        self.storage
            .save_version_vector(&self.version_vector)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

        Ok(())
    }

    /// Returns pending sync items for a specific device.
    pub fn pending_for_device(&self, device_id: &[u8; 32]) -> &[SyncItem] {
        self.device_states
            .get(device_id)
            .map(|s| s.pending_items())
            .unwrap_or(&[])
    }

    /// Returns all device IDs that have pending items.
    pub fn devices_with_pending(&self) -> Vec<[u8; 32]> {
        self.device_states
            .iter()
            .filter(|(_, state)| !state.pending_items().is_empty())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Marks items as synced to a device.
    pub fn mark_synced(
        &mut self,
        device_id: &[u8; 32],
        version: u64,
    ) -> Result<(), DeviceSyncError> {
        if let Some(state) = self.device_states.get_mut(device_id) {
            state.mark_synced(version);
            self.storage
                .save_device_sync_state(state)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }
        Ok(())
    }

    /// Creates a full sync payload for a newly linked device.
    ///
    /// This includes all contacts and the user's own contact card.
    pub fn create_full_sync_payload(&self) -> Result<DeviceSyncPayload, DeviceSyncError> {
        // Load contacts from storage
        let contacts = self
            .storage
            .list_contacts()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        // Load own card from storage
        let own_card = self
            .storage
            .load_own_card()
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?
            .unwrap_or_else(|| ContactCard::new(""));

        // Get current version
        let version = self.version_vector.get(self.current_device.device_id());

        Ok(DeviceSyncPayload::new(&contacts, &own_card, version))
    }

    /// Applies a full sync payload received during device linking.
    ///
    /// This replaces the local state with the received data.
    pub fn apply_full_sync(&mut self, payload: DeviceSyncPayload) -> Result<(), DeviceSyncError> {
        // Save own card
        if !payload.own_card_json.is_empty() {
            let own_card: ContactCard = serde_json::from_str(&payload.own_card_json)
                .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;
            self.storage
                .save_own_card(&own_card)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }

        // Save contacts
        for contact_data in &payload.contacts {
            let contact = contact_data.to_contact()?;
            self.storage
                .save_contact(&contact)
                .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        }

        // Update version vector to match received version
        self.version_vector
            .increment(self.current_device.device_id());

        self.storage
            .save_version_vector(&self.version_vector)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;

        Ok(())
    }

    /// Returns the current device info.
    pub fn current_device(&self) -> &DeviceInfo {
        &self.current_device
    }

    /// Returns the device registry.
    pub fn registry(&self) -> &DeviceRegistry {
        &self.registry
    }

    /// Returns the local version vector.
    pub fn version_vector(&self) -> &VersionVector {
        &self.version_vector
    }

    /// Adds a new device to track (called after device linking).
    pub fn add_device(&mut self, device_id: [u8; 32]) {
        self.device_states
            .entry(device_id)
            .or_insert_with(|| InterDeviceSyncState::new(device_id));
    }

    /// Removes a device from tracking (called after device revocation).
    pub fn remove_device(&mut self, device_id: &[u8; 32]) -> Result<(), DeviceSyncError> {
        self.device_states.remove(device_id);
        self.storage
            .delete_device_sync_state(device_id)
            .map_err(|e| DeviceSyncError::Serialization(e.to_string()))?;
        Ok(())
    }

    // ============================================================
    // Device-to-device encryption (Phase 4)
    // ============================================================

    /// Encrypts data for another device using ECDH + AES-GCM.
    ///
    /// Uses the current device's exchange key to perform ECDH with the target
    /// device's public key, derives an encryption key via HKDF, and encrypts
    /// the data with AES-256-GCM.
    pub fn encrypt_for_device(
        &self,
        target_public_key: &[u8; 32],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, DeviceSyncError> {
        let encryption_key = self.derive_shared_key(target_public_key);
        encryption::encrypt(&encryption_key, plaintext)
            .map_err(|e| DeviceSyncError::Encryption(e.to_string()))
    }

    /// Decrypts data from another device using ECDH + AES-GCM.
    ///
    /// Uses the current device's exchange key to perform ECDH with the sender
    /// device's public key, derives a decryption key via HKDF, and decrypts
    /// the data with AES-256-GCM.
    pub fn decrypt_from_device(
        &self,
        sender_public_key: &[u8; 32],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, DeviceSyncError> {
        let decryption_key = self.derive_shared_key(sender_public_key);
        encryption::decrypt(&decryption_key, ciphertext)
            .map_err(|e| DeviceSyncError::Decryption(e.to_string()))
    }

    /// Derives a shared symmetric key from ECDH with another device.
    fn derive_shared_key(&self, their_public_key: &[u8; 32]) -> SymmetricKey {
        // ECDH: our_secret * their_public -> shared_secret
        let shared_secret = self
            .current_device
            .exchange_keypair()
            .diffie_hellman(their_public_key);

        // HKDF to derive encryption key
        let key_bytes = HKDF::derive_key(None, &shared_secret, DEVICE_SYNC_INFO);
        SymmetricKey::from_bytes(key_bytes)
    }

    // ============================================================
    // Conflict Resolution (Phase 5)
    // ============================================================

    /// Processes incoming sync items from another device.
    ///
    /// Uses last-write-wins conflict resolution:
    /// - If incoming item has a newer timestamp than local, apply it
    /// - If incoming item has an older timestamp, reject it
    /// - Different fields/items don't conflict
    ///
    /// Returns the list of items that were applied.
    pub fn process_incoming(&mut self, items: Vec<SyncItem>) -> Result<Vec<SyncItem>, DeviceSyncError> {
        let mut applied = Vec::new();

        for item in items {
            let key = Self::conflict_key(&item);
            let incoming_timestamp = item.timestamp();

            // Check if we have a local timestamp for this key
            let local_timestamp = self.field_timestamps.get(&key).copied().unwrap_or(0);

            // Last-write-wins: only apply if incoming is newer
            if incoming_timestamp > local_timestamp {
                // Update our local timestamp
                self.field_timestamps.insert(key, incoming_timestamp);

                // Add to applied list
                applied.push(item);
            }
            // If incoming is older or equal, we reject it (don't add to applied)
        }

        Ok(applied)
    }

    /// Generates a conflict key for a SyncItem.
    ///
    /// Items with the same key are considered conflicting (only one can win).
    /// Items with different keys are independent and can both be applied.
    fn conflict_key(item: &SyncItem) -> String {
        match item {
            SyncItem::ContactAdded { contact_data, .. } => format!("contact:{}", contact_data.id),
            SyncItem::ContactRemoved { contact_id, .. } => format!("contact:{}", contact_id),
            SyncItem::CardUpdated { field_label, .. } => format!("field:{}", field_label),
            SyncItem::VisibilityChanged { contact_id, .. } => format!("visibility:{}", contact_id),
        }
    }

    // ============================================================
    // Bidirectional Sync (Phase 6)
    // ============================================================

    /// Creates a sync message containing all pending items for a target device.
    ///
    /// This is used when reconnecting to send all queued changes to another device.
    pub fn create_sync_message(&self, device_id: &[u8; 32]) -> Result<PendingSyncMessage, DeviceSyncError> {
        let items = self.pending_for_device(device_id).to_vec();
        let version = self.version_vector.get(self.current_device.device_id());

        Ok(PendingSyncMessage {
            items,
            version,
            sender_device_id: *self.current_device.device_id(),
        })
    }
}

/// A message containing pending sync items to send to another device.
#[derive(Debug, Clone)]
pub struct PendingSyncMessage {
    /// The pending sync items.
    pub items: Vec<SyncItem>,
    /// Version number for deduplication.
    pub version: u64,
    /// The sender device ID.
    pub sender_device_id: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact::Contact;
    use crate::contact_card::{ContactCard, ContactField, FieldType};
    use crate::crypto::{SigningKeyPair, SymmetricKey};

    fn create_test_storage() -> Storage {
        let key = SymmetricKey::generate();
        Storage::in_memory(key).unwrap()
    }

    fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
        DeviceInfo::derive(master_seed, index, name.to_string())
    }

    fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
        let signing_key = SigningKeyPair::from_seed(master_seed);
        DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
    }

    fn create_test_contact(name: &str) -> Contact {
        let public_key = [0x42u8; 32];
        let card = ContactCard::new(name);
        let shared_key = SymmetricKey::generate();
        Contact::from_exchange(public_key, card, shared_key)
    }

    // ============================================================
    // Phase 3: Device Sync Orchestrator Tests (TDD)
    // Based on features/device_management.feature @sync scenarios
    // ============================================================

    /// Scenario: Changes sync between devices
    /// "When I update my phone number on Device A
    ///  Then Device B should receive the update"
    #[test]
    fn test_orchestrator_record_local_change() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        // Create two devices
        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");

        // Create registry with both devices
        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        // Create orchestrator on Device A
        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Record a local change
        let item = SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1234567890".to_string(),
            timestamp: 1000,
        };
        orchestrator.record_local_change(item).unwrap();

        // Verify the change is queued for Device B
        let pending = orchestrator.pending_for_device(device_b.device_id());
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].timestamp(), 1000);
    }

    /// Test that pending items returns correct results
    #[test]
    fn test_orchestrator_pending_for_device() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_id = *device_b.device_id();

        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Initially no pending items
        assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 0);

        // Add some changes
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "test@example.com".to_string(),
                timestamp: 1000,
            })
            .unwrap();

        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "phone".to_string(),
                new_value: "+999".to_string(),
                timestamp: 2000,
            })
            .unwrap();

        // Now should have 2 pending items
        assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 2);
    }

    /// Scenario: New device receives full state
    /// "When Device B is newly linked
    ///  Then Device B should receive my complete contact card
    ///  And Device B should receive all my contacts"
    #[test]
    fn test_orchestrator_create_full_sync_payload() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];

        let device_a = create_test_device(&master_seed, 0, "Device A");
        let registry = create_test_registry(&master_seed, &device_a);

        // Add some contacts and own card to storage
        let mut own_card = ContactCard::new("Alice");
        let _ = own_card.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "alice@example.com",
        ));
        storage.save_own_card(&own_card).unwrap();

        let contact = create_test_contact("Bob");
        storage.save_contact(&contact).unwrap();

        // Create orchestrator
        let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Create full sync payload
        let payload = orchestrator.create_full_sync_payload().unwrap();

        assert_eq!(payload.contact_count(), 1);
        assert!(!payload.own_card_json.is_empty());
    }

    /// Scenario: New device applies received state
    #[test]
    fn test_orchestrator_apply_full_sync() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];

        let device_b = create_test_device(&master_seed, 1, "Device B");
        let registry = create_test_registry(&master_seed, &device_b);

        // Create orchestrator for new device (Device B)
        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

        // Create a sync payload (simulating what Device A would send)
        let own_card = ContactCard::new("Alice");
        let contact = create_test_contact("Bob");
        let payload = DeviceSyncPayload::new(&[contact], &own_card, 1);

        // Apply the sync payload
        orchestrator.apply_full_sync(payload).unwrap();

        // Verify own card was saved
        let loaded_card = storage.load_own_card().unwrap().unwrap();
        assert_eq!(loaded_card.display_name(), "Alice");

        // Verify contact was saved
        let contacts = storage.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].display_name(), "Bob");
    }

    /// Test marking items as synced clears pending queue
    #[test]
    fn test_orchestrator_mark_synced() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_id = *device_b.device_id();

        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Add a change
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "test@example.com".to_string(),
                timestamp: 1000,
            })
            .unwrap();

        assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 1);

        // Mark as synced
        orchestrator.mark_synced(&device_b_id, 1).unwrap();

        // Now should be empty
        assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 0);
    }

    /// Test version vector is incremented on local changes
    #[test]
    fn test_orchestrator_version_vector_increment() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];

        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_a_id = *device_a.device_id();
        let registry = create_test_registry(&master_seed, &device_a);

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Initially version is 0
        assert_eq!(orchestrator.version_vector().get(&device_a_id), 0);

        // Record a change
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "test@example.com".to_string(),
                timestamp: 1000,
            })
            .unwrap();

        // Version should be incremented
        assert_eq!(orchestrator.version_vector().get(&device_a_id), 1);
    }

    /// Test loading state from storage
    #[test]
    fn test_orchestrator_load_persisted_state() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        // Get device_b_id first before consuming device_b
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_id = *device_b.device_id();

        // Create orchestrator and add some changes
        {
            let device_a = create_test_device(&master_seed, 0, "Device A");
            let mut registry = create_test_registry(&master_seed, &device_a);
            registry
                .add_device(device_b.to_registered(&master_seed), &signing_key)
                .unwrap();

            let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);
            orchestrator
                .record_local_change(SyncItem::CardUpdated {
                    field_label: "email".to_string(),
                    new_value: "test@example.com".to_string(),
                    timestamp: 1000,
                })
                .unwrap();
        }

        // Create new instances for loading
        let device_a2 = create_test_device(&master_seed, 0, "Device A");
        let device_b2 = create_test_device(&master_seed, 1, "Device B");
        let mut registry2 = create_test_registry(&master_seed, &device_a2);
        registry2
            .add_device(device_b2.to_registered(&master_seed), &signing_key)
            .unwrap();

        // Load state from storage
        let orchestrator = DeviceSyncOrchestrator::load(&storage, device_a2, registry2).unwrap();

        // Should still have the pending item
        assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 1);
    }

    // ============================================================
    // Phase 4: Encryption Layer Tests (TDD)
    // Device-to-device encryption using ECDH + AES-GCM
    // ============================================================

    /// Test encrypting data for another device
    /// Uses ECDH: our_secret * their_public -> shared_secret
    /// Then HKDF to derive encryption key
    #[test]
    fn test_encrypt_for_device() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_public_key = *device_b.exchange_public_key();

        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Encrypt some data for device B
        let plaintext = b"Hello from Device A!";
        let ciphertext = orchestrator
            .encrypt_for_device(&device_b_public_key, plaintext)
            .unwrap();

        // Ciphertext should be different from plaintext
        assert_ne!(ciphertext, plaintext);
        // Ciphertext should be longer (includes nonce + tag)
        assert!(ciphertext.len() > plaintext.len());
    }

    /// Test decrypting data from another device
    #[test]
    fn test_decrypt_from_device() {
        let storage_a = create_test_storage();
        let storage_b = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        // Create both devices
        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_a_public_key = *device_a.exchange_public_key();
        let device_b_public_key = *device_b.exchange_public_key();

        // Registry for device A
        let mut registry_a = create_test_registry(&master_seed, &device_a);
        registry_a
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        // Registry for device B
        let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
        let device_b_for_b = create_test_device(&master_seed, 1, "Device B");
        let mut registry_b = create_test_registry(&master_seed, &device_b_for_b);
        registry_b
            .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
        let orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b_for_b, registry_b);

        // Device A encrypts for Device B
        let plaintext = b"Secret message from A to B";
        let ciphertext = orchestrator_a
            .encrypt_for_device(&device_b_public_key, plaintext)
            .unwrap();

        // Device B decrypts from Device A
        let decrypted = orchestrator_b
            .decrypt_from_device(&device_a_public_key, &ciphertext)
            .unwrap();

        assert_eq!(decrypted, plaintext);
    }

    /// Test that wrong device cannot decrypt
    #[test]
    fn test_wrong_device_cannot_decrypt() {
        let storage_a = create_test_storage();
        let storage_c = create_test_storage();
        let master_seed = [0x42u8; 32];
        let different_seed = [0x99u8; 32]; // Different identity
        let signing_key = SigningKeyPair::from_seed(&master_seed);
        let _signing_key_c = SigningKeyPair::from_seed(&different_seed);

        // Create devices A and B (same identity)
        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_public_key = *device_b.exchange_public_key();

        // Create device C (different identity - attacker)
        let device_c = create_test_device(&different_seed, 0, "Device C");

        // Registry for device A
        let mut registry_a = create_test_registry(&master_seed, &device_a);
        registry_a
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        // Registry for device C (pretending it has A in registry)
        let registry_c = create_test_registry(&different_seed, &device_c);

        let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
        let orchestrator_c = DeviceSyncOrchestrator::new(&storage_c, device_c, registry_c);

        // Device A encrypts for Device B
        let plaintext = b"Secret message for B only";
        let ciphertext = orchestrator_a
            .encrypt_for_device(&device_b_public_key, plaintext)
            .unwrap();

        // Device C (attacker) tries to decrypt - should fail
        // Even if C knows A's public key, C doesn't have B's secret key
        let device_a_public_key = *create_test_device(&master_seed, 0, "Device A").exchange_public_key();
        let result = orchestrator_c.decrypt_from_device(&device_a_public_key, &ciphertext);

        assert!(result.is_err());
    }

    // ============================================================
    // Phase 5: Conflict Resolution Tests (TDD)
    // Based on features/device_management.feature @sync scenarios
    // ============================================================

    /// Scenario: Conflict resolution between devices
    /// "Given I have made conflicting changes on Device A and Device B
    ///  Then the most recent change should win"
    #[test]
    fn test_conflict_resolution_last_write_wins() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];

        let device_b = create_test_device(&master_seed, 1, "Device B");
        let registry = create_test_registry(&master_seed, &device_b);

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

        // Device B has a local change with timestamp 1000
        let local_item = SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "local@example.com".to_string(),
            timestamp: 1000,
        };
        orchestrator.record_local_change(local_item).unwrap();

        // Incoming change from Device A with timestamp 2000 (newer)
        let incoming_items = vec![SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "remote@example.com".to_string(),
            timestamp: 2000,
        }];

        // Process incoming items
        let applied = orchestrator.process_incoming(incoming_items).unwrap();

        // The newer remote change should be applied
        assert_eq!(applied.len(), 1);
        match &applied[0] {
            SyncItem::CardUpdated { new_value, .. } => {
                assert_eq!(new_value, "remote@example.com");
            }
            _ => panic!("Expected CardUpdated"),
        }
    }

    /// Test that older incoming changes are rejected
    #[test]
    fn test_conflict_resolution_rejects_older() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];

        let device_b = create_test_device(&master_seed, 1, "Device B");
        let registry = create_test_registry(&master_seed, &device_b);

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

        // Device B has a local change with timestamp 2000
        let local_item = SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "newer_local@example.com".to_string(),
            timestamp: 2000,
        };
        orchestrator.record_local_change(local_item).unwrap();

        // Incoming change from Device A with timestamp 1000 (older)
        let incoming_items = vec![SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "older_remote@example.com".to_string(),
            timestamp: 1000,
        }];

        // Process incoming items
        let applied = orchestrator.process_incoming(incoming_items).unwrap();

        // The older remote change should be rejected (empty applied list)
        assert!(applied.is_empty());
    }

    /// Scenario: Bidirectional sync
    /// "When I add a phone number on Device A
    ///  And I add an email on Device B
    ///  Then both devices should have both fields"
    #[test]
    fn test_concurrent_updates_different_fields_both_preserved() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];

        let device_b = create_test_device(&master_seed, 1, "Device B");
        let registry = create_test_registry(&master_seed, &device_b);

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

        // Device B adds email locally
        let local_item = SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "b@example.com".to_string(),
            timestamp: 1000,
        };
        orchestrator.record_local_change(local_item).unwrap();

        // Device A added phone at roughly the same time
        let incoming_items = vec![SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1234567890".to_string(),
            timestamp: 1001,
        }];

        // Process incoming - different fields, no conflict
        let applied = orchestrator.process_incoming(incoming_items).unwrap();

        // The phone update should be applied (different field)
        assert_eq!(applied.len(), 1);
        match &applied[0] {
            SyncItem::CardUpdated { field_label, new_value, .. } => {
                assert_eq!(field_label, "phone");
                assert_eq!(new_value, "+1234567890");
            }
            _ => panic!("Expected CardUpdated"),
        }
    }

    // ============================================================
    // Phase 6: Bidirectional Sync Tests (TDD)
    // Based on features/device_management.feature @sync scenarios
    // ============================================================

    /// Scenario: Bidirectional sync with merge
    /// Both devices add different fields; both should end up with both
    #[test]
    fn test_bidirectional_field_additions() {
        let storage_a = create_test_storage();
        let storage_b = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        // Set up Device A
        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b_for_a = create_test_device(&master_seed, 1, "Device B");
        let device_b_id = *device_b_for_a.device_id();
        let mut registry_a = create_test_registry(&master_seed, &device_a);
        registry_a
            .add_device(device_b_for_a.to_registered(&master_seed), &signing_key)
            .unwrap();

        // Set up Device B
        let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_a_id = *device_a_for_b.device_id();
        let mut registry_b = create_test_registry(&master_seed, &device_b);
        registry_b
            .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let mut orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
        let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);

        // Device A adds phone
        orchestrator_a
            .record_local_change(SyncItem::CardUpdated {
                field_label: "phone".to_string(),
                new_value: "+1111111111".to_string(),
                timestamp: 1000,
            })
            .unwrap();

        // Device B adds email
        orchestrator_b
            .record_local_change(SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "user@example.com".to_string(),
                timestamp: 1001,
            })
            .unwrap();

        // Exchange pending items
        let a_to_b = orchestrator_a.pending_for_device(&device_b_id).to_vec();
        let b_to_a = orchestrator_b.pending_for_device(&device_a_id).to_vec();

        // Apply on each side
        let applied_on_b = orchestrator_b.process_incoming(a_to_b).unwrap();
        let applied_on_a = orchestrator_a.process_incoming(b_to_a).unwrap();

        // Both should have applied the other's changes (different fields, no conflict)
        assert_eq!(applied_on_b.len(), 1); // phone from A
        assert_eq!(applied_on_a.len(), 1); // email from B
    }

    /// Scenario: Offline changes are queued
    /// Changes made while offline should be stored for later sync
    #[test]
    fn test_offline_changes_queue() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_id = *device_b.device_id();

        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Make multiple offline changes
        for i in 1..=5 {
            orchestrator
                .record_local_change(SyncItem::CardUpdated {
                    field_label: format!("field_{}", i),
                    new_value: format!("value_{}", i),
                    timestamp: i * 1000,
                })
                .unwrap();
        }

        // All changes should be queued for Device B
        let pending = orchestrator.pending_for_device(&device_b_id);
        assert_eq!(pending.len(), 5);
    }

    /// Scenario: Offline changes sync when reconnected
    /// "Given Device B is offline
    ///  When Device B makes changes offline
    ///  And Device B reconnects
    ///  Then those changes should sync to Device A"
    #[test]
    fn test_offline_changes_sync_on_reconnect() {
        let storage = create_test_storage();
        let master_seed = [0x42u8; 32];
        let signing_key = SigningKeyPair::from_seed(&master_seed);

        let device_a = create_test_device(&master_seed, 0, "Device A");
        let device_b = create_test_device(&master_seed, 1, "Device B");
        let device_b_id = *device_b.device_id();

        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        // Create orchestrator
        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

        // Make offline changes
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "offline_field".to_string(),
                new_value: "offline_value".to_string(),
                timestamp: 5000,
            })
            .unwrap();

        // Verify the change is in pending queue
        let pending = orchestrator.pending_for_device(&device_b_id);
        assert_eq!(pending.len(), 1);

        // Create sync message for reconnection
        let sync_message = orchestrator.create_sync_message(&device_b_id).unwrap();

        // Verify sync message contains the pending items
        assert!(!sync_message.items.is_empty());
        assert_eq!(sync_message.items.len(), 1);
    }
}
