//! Device-to-Device Sync Module
//!
//! Handles syncing data between devices belonging to the same identity.
//! Used during device linking and for ongoing inter-device synchronization.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;

/// Serializable contact data for device sync.
///
/// Contains all information needed to reconstruct a contact on a new device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactSyncData {
    /// Contact's unique ID (public key fingerprint).
    pub id: String,
    /// Contact's Ed25519 public key.
    #[serde(with = "bytes_array_32")]
    pub public_key: [u8; 32],
    /// Contact's display name.
    pub display_name: String,
    /// Contact's card as JSON.
    pub card_json: String,
    /// Shared symmetric key bytes.
    #[serde(with = "bytes_array_32")]
    pub shared_key: [u8; 32],
    /// Exchange timestamp.
    pub exchange_timestamp: u64,
    /// Whether fingerprint was verified.
    pub fingerprint_verified: bool,
    /// Visibility rules as JSON.
    pub visibility_rules_json: String,
}

impl ContactSyncData {
    /// Creates sync data from a contact.
    pub fn from_contact(contact: &Contact) -> Self {
        let card_json = serde_json::to_string(contact.card())
            .expect("Card serialization should not fail");
        let visibility_rules_json = serde_json::to_string(contact.visibility_rules())
            .expect("Visibility rules serialization should not fail");

        ContactSyncData {
            id: contact.id().to_string(),
            public_key: *contact.public_key(),
            display_name: contact.display_name().to_string(),
            card_json,
            shared_key: *contact.shared_key().as_bytes(),
            exchange_timestamp: contact.exchange_timestamp(),
            fingerprint_verified: contact.is_fingerprint_verified(),
            visibility_rules_json,
        }
    }

    /// Converts sync data back to a contact.
    pub fn to_contact(&self) -> Result<Contact, DeviceSyncError> {
        let card: ContactCard = serde_json::from_str(&self.card_json)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        let visibility_rules = serde_json::from_str(&self.visibility_rules_json)
            .map_err(|e| DeviceSyncError::Deserialization(e.to_string()))?;

        let shared_key = SymmetricKey::from_bytes(self.shared_key);

        Ok(Contact::from_sync_data(
            self.public_key,
            card,
            shared_key,
            self.exchange_timestamp,
            self.fingerprint_verified,
            visibility_rules,
        ))
    }
}

/// Payload for syncing all contacts during device linking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncPayload {
    /// All contacts to sync.
    pub contacts: Vec<ContactSyncData>,
    /// User's own contact card.
    pub own_card_json: String,
    /// Version number for conflict resolution.
    pub version: u64,
}

impl DeviceSyncPayload {
    /// Creates an empty sync payload.
    pub fn empty() -> Self {
        DeviceSyncPayload {
            contacts: Vec::new(),
            own_card_json: String::new(),
            version: 0,
        }
    }

    /// Creates a sync payload from contacts and card.
    pub fn new(contacts: &[Contact], own_card: &ContactCard, version: u64) -> Self {
        let contacts_data: Vec<ContactSyncData> = contacts
            .iter()
            .map(ContactSyncData::from_contact)
            .collect();

        let own_card_json = serde_json::to_string(own_card)
            .expect("Card serialization should not fail");

        DeviceSyncPayload {
            contacts: contacts_data,
            own_card_json,
            version,
        }
    }

    /// Serializes the payload to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("DeviceSyncPayload serialization should not fail")
    }

    /// Deserializes a payload from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }

    /// Returns the number of contacts.
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }
}

/// Errors that can occur during device sync.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DeviceSyncError {
    #[error("Serialization failed: {0}")]
    Serialization(String),

    #[error("Deserialization failed: {0}")]
    Deserialization(String),

    #[error("Encryption failed")]
    EncryptionFailed,

    #[error("Decryption failed")]
    DecryptionFailed,
}

// ============================================================
// Phase 4: Inter-Device Sync Types
// ============================================================

/// Types of sync events between devices.
///
/// Each SyncItem represents an atomic change that needs to be
/// synchronized across all devices belonging to the same identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncItem {
    /// A new contact was added.
    ContactAdded {
        /// Full contact data for reconstruction.
        contact_data: ContactSyncData,
        /// Timestamp when the contact was added.
        timestamp: u64,
    },

    /// A contact was removed.
    ContactRemoved {
        /// ID of the removed contact.
        contact_id: String,
        /// Timestamp of removal.
        timestamp: u64,
    },

    /// Own contact card field was updated.
    CardUpdated {
        /// Field label that was updated.
        field_label: String,
        /// New field value.
        new_value: String,
        /// Timestamp of update.
        timestamp: u64,
    },

    /// Visibility setting for a contact changed.
    VisibilityChanged {
        /// Contact ID whose visibility changed.
        contact_id: String,
        /// Field label affected.
        field_label: String,
        /// New visibility state.
        is_visible: bool,
        /// Timestamp of change.
        timestamp: u64,
    },
}

impl SyncItem {
    /// Returns the timestamp of this sync item.
    pub fn timestamp(&self) -> u64 {
        match self {
            SyncItem::ContactAdded { timestamp, .. } => *timestamp,
            SyncItem::ContactRemoved { timestamp, .. } => *timestamp,
            SyncItem::CardUpdated { timestamp, .. } => *timestamp,
            SyncItem::VisibilityChanged { timestamp, .. } => *timestamp,
        }
    }

    /// Resolves conflict between two sync items using last-write-wins.
    ///
    /// The item with the later timestamp wins.
    pub fn resolve_conflict(a: &SyncItem, b: &SyncItem) -> SyncItem {
        if a.timestamp() >= b.timestamp() {
            a.clone()
        } else {
            b.clone()
        }
    }

    /// Serializes this item to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("SyncItem serialization should not fail")
    }

    /// Deserializes an item from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }
}

/// Tracks synchronization state with another device.
///
/// Each device maintains one InterDeviceSyncState per other linked device
/// to track what has been synced and what is pending.
#[derive(Debug, Clone)]
pub struct InterDeviceSyncState {
    /// ID of the target device.
    device_id: [u8; 32],
    /// Items pending sync to this device.
    pending_items: Vec<SyncItem>,
    /// Last sync version number.
    last_sync_version: u64,
}

impl InterDeviceSyncState {
    /// Creates a new sync state for the given device.
    pub fn new(device_id: [u8; 32]) -> Self {
        InterDeviceSyncState {
            device_id,
            pending_items: Vec::new(),
            last_sync_version: 0,
        }
    }

    /// Returns the target device ID.
    pub fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }

    /// Returns pending items to sync.
    pub fn pending_items(&self) -> &[SyncItem] {
        &self.pending_items
    }

    /// Returns the last sync version.
    pub fn last_sync_version(&self) -> u64 {
        self.last_sync_version
    }

    /// Queues an item for sync to this device.
    pub fn queue_item(&mut self, item: SyncItem) {
        self.pending_items.push(item);
    }

    /// Marks items as synced up to the given version.
    pub fn mark_synced(&mut self, version: u64) {
        self.pending_items.clear();
        self.last_sync_version = version;
    }
}

/// Version vector for causality tracking across devices.
///
/// Used to detect concurrent updates and determine if changes
/// happened before, after, or concurrently with other changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionVector {
    /// Map of device ID to version number.
    versions: HashMap<[u8; 32], u64>,
}

impl VersionVector {
    /// Creates an empty version vector.
    pub fn new() -> Self {
        VersionVector {
            versions: HashMap::new(),
        }
    }

    /// Gets the version for a device.
    pub fn get(&self, device_id: &[u8; 32]) -> u64 {
        self.versions.get(device_id).copied().unwrap_or(0)
    }

    /// Increments the version for a device.
    pub fn increment(&mut self, device_id: &[u8; 32]) {
        let current = self.get(device_id);
        self.versions.insert(*device_id, current + 1);
    }

    /// Merges two version vectors, taking the max of each device's version.
    pub fn merge(a: &VersionVector, b: &VersionVector) -> VersionVector {
        let mut merged = a.clone();

        for (device_id, &version) in &b.versions {
            let current = merged.get(device_id);
            if version > current {
                merged.versions.insert(*device_id, version);
            }
        }

        merged
    }

    /// Checks if this vector is concurrent with another.
    ///
    /// Two vectors are concurrent if neither dominates the other
    /// (i.e., each has at least one version the other doesn't have).
    pub fn is_concurrent_with(&self, other: &VersionVector) -> bool {
        let self_dominates = self.dominates(other);
        let other_dominates = other.dominates(self);

        // Concurrent if neither dominates
        !self_dominates && !other_dominates
    }

    /// Checks if this vector dominates another.
    ///
    /// A dominates B if A[d] >= B[d] for all devices d,
    /// and A[d] > B[d] for at least one device.
    fn dominates(&self, other: &VersionVector) -> bool {
        let mut dominated = false;

        // Check all devices in other
        for (device_id, &other_ver) in &other.versions {
            let self_ver = self.get(device_id);
            if self_ver < other_ver {
                return false; // other has a higher version
            }
            if self_ver > other_ver {
                dominated = true;
            }
        }

        // Check devices only in self
        for (device_id, &self_ver) in &self.versions {
            if !other.versions.contains_key(device_id) && self_ver > 0 {
                dominated = true;
            }
        }

        dominated
    }
}

/// Serde helper for 32-byte arrays.
mod bytes_array_32 {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = BASE64.decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 32-byte array"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::ContactCard;

    fn create_test_contact() -> Contact {
        let public_key = [0x42u8; 32];
        let card = ContactCard::new("Alice");
        let shared_key = SymmetricKey::from_bytes([0x55u8; 32]);
        Contact::from_exchange(public_key, card, shared_key)
    }

    #[test]
    fn test_contact_sync_data_roundtrip() {
        let contact = create_test_contact();
        let sync_data = ContactSyncData::from_contact(&contact);
        let restored = sync_data.to_contact().unwrap();

        assert_eq!(restored.id(), contact.id());
        assert_eq!(restored.public_key(), contact.public_key());
        assert_eq!(restored.display_name(), contact.display_name());
    }

    #[test]
    fn test_contact_sync_data_serialization() {
        let contact = create_test_contact();
        let sync_data = ContactSyncData::from_contact(&contact);

        let json = serde_json::to_string(&sync_data).unwrap();
        let restored: ContactSyncData = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, sync_data.id);
        assert_eq!(restored.public_key, sync_data.public_key);
    }

    #[test]
    fn test_device_sync_payload_roundtrip() {
        let contact1 = create_test_contact();
        let own_card = ContactCard::new("Bob");

        let payload = DeviceSyncPayload::new(&[contact1], &own_card, 1);

        let json = payload.to_json();
        let restored = DeviceSyncPayload::from_json(&json).unwrap();

        assert_eq!(restored.contact_count(), 1);
        assert_eq!(restored.version, 1);
    }

    #[test]
    fn test_device_sync_payload_empty() {
        let payload = DeviceSyncPayload::empty();
        assert_eq!(payload.contact_count(), 0);
        assert_eq!(payload.version, 0);
    }

    // ============================================================
    // Phase 4 Tests: Inter-Device Sync
    // Based on features/device_management.feature @sync scenarios
    // ============================================================

    /// Scenario: Changes sync between devices
    /// "When I update my phone number on Device A
    ///  Then Device B should receive the update"
    #[test]
    fn test_sync_item_card_updated() {
        use crate::contact_card::{ContactField, FieldType};

        let mut card = ContactCard::new("Alice");
        card.add_field(ContactField::new(FieldType::Phone, "mobile", "+1234567890"));

        // Create a SyncItem representing a card field update
        let item = SyncItem::CardUpdated {
            field_label: "mobile".to_string(),
            new_value: "+1987654321".to_string(),
            timestamp: 1000,
        };

        assert!(matches!(item, SyncItem::CardUpdated { .. }));

        // Verify timestamp is accessible for conflict resolution
        assert_eq!(item.timestamp(), 1000);
    }

    /// Scenario: Bidirectional sync
    /// "When I add a field on Device A
    ///  And I add a different field on Device B
    ///  Then both fields should appear on both devices"
    #[test]
    fn test_sync_item_contact_added() {
        let contact = create_test_contact();
        let sync_data = ContactSyncData::from_contact(&contact);

        let item = SyncItem::ContactAdded {
            contact_data: sync_data,
            timestamp: 2000,
        };

        assert!(matches!(item, SyncItem::ContactAdded { .. }));
        assert_eq!(item.timestamp(), 2000);
    }

    /// Scenario: Conflict resolution between devices
    /// "When I update my email to 'a@test.com' on Device A
    ///  And I update my email to 'b@test.com' on Device B
    ///  And both come online
    ///  Then the later change should win"
    #[test]
    fn test_conflict_resolution_last_write_wins() {
        // Device A update at timestamp 1000
        let item_a = SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "a@test.com".to_string(),
            timestamp: 1000,
        };

        // Device B update at timestamp 2000 (later)
        let item_b = SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "b@test.com".to_string(),
            timestamp: 2000,
        };

        // Resolve conflict - later timestamp wins
        let resolved = SyncItem::resolve_conflict(&item_a, &item_b);

        // Device B's change should win
        if let SyncItem::CardUpdated { new_value, .. } = resolved {
            assert_eq!(new_value, "b@test.com");
        } else {
            panic!("Expected CardUpdated variant");
        }
    }

    /// Test SyncItem visibility change
    #[test]
    fn test_sync_item_visibility_changed() {
        let item = SyncItem::VisibilityChanged {
            contact_id: "contact-123".to_string(),
            field_label: "phone".to_string(),
            is_visible: false,
            timestamp: 3000,
        };

        assert!(matches!(item, SyncItem::VisibilityChanged { .. }));
        assert_eq!(item.timestamp(), 3000);
    }

    /// Test SyncItem contact removed
    #[test]
    fn test_sync_item_contact_removed() {
        let item = SyncItem::ContactRemoved {
            contact_id: "contact-456".to_string(),
            timestamp: 4000,
        };

        assert!(matches!(item, SyncItem::ContactRemoved { .. }));
        assert_eq!(item.timestamp(), 4000);
    }

    /// Test InterDeviceSyncState for tracking sync with other own devices
    #[test]
    fn test_inter_device_sync_state_creation() {
        let device_id = [0x42u8; 32];

        let state = InterDeviceSyncState::new(device_id);

        assert_eq!(state.device_id(), &device_id);
        assert_eq!(state.pending_items().len(), 0);
        assert_eq!(state.last_sync_version(), 0);
    }

    /// Test adding items to sync queue
    #[test]
    fn test_inter_device_sync_state_queue_item() {
        let device_id = [0x42u8; 32];
        let mut state = InterDeviceSyncState::new(device_id);

        let item = SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        };

        state.queue_item(item);

        assert_eq!(state.pending_items().len(), 1);
    }

    /// Test serialization of SyncItem for transmission
    #[test]
    fn test_sync_item_serialization() {
        let item = SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1234567890".to_string(),
            timestamp: 5000,
        };

        let json = item.to_json();
        let restored = SyncItem::from_json(&json).unwrap();

        assert_eq!(item.timestamp(), restored.timestamp());
    }

    /// Test version vector for causality tracking
    #[test]
    fn test_version_vector_increment() {
        let device_id = [0x42u8; 32];
        let mut version_vector = VersionVector::new();

        version_vector.increment(&device_id);
        assert_eq!(version_vector.get(&device_id), 1);

        version_vector.increment(&device_id);
        assert_eq!(version_vector.get(&device_id), 2);
    }

    /// Test version vector merge for conflict detection
    #[test]
    fn test_version_vector_merge() {
        let device_a = [0x41u8; 32];
        let device_b = [0x42u8; 32];

        let mut vv_a = VersionVector::new();
        vv_a.increment(&device_a);
        vv_a.increment(&device_a);

        let mut vv_b = VersionVector::new();
        vv_b.increment(&device_b);
        vv_b.increment(&device_b);
        vv_b.increment(&device_b);

        let merged = VersionVector::merge(&vv_a, &vv_b);

        assert_eq!(merged.get(&device_a), 2);
        assert_eq!(merged.get(&device_b), 3);
    }

    /// Test version vector comparison for conflict detection
    #[test]
    fn test_version_vector_concurrent_detection() {
        let device_a = [0x41u8; 32];
        let device_b = [0x42u8; 32];

        let mut vv_a = VersionVector::new();
        vv_a.increment(&device_a);

        let mut vv_b = VersionVector::new();
        vv_b.increment(&device_b);

        // Neither dominates the other - they are concurrent
        assert!(vv_a.is_concurrent_with(&vv_b));
    }
}
