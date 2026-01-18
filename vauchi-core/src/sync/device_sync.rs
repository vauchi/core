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
        let card_json =
            serde_json::to_string(contact.card()).expect("Card serialization should not fail");
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
        let contacts_data: Vec<ContactSyncData> =
            contacts.iter().map(ContactSyncData::from_contact).collect();

        let own_card_json =
            serde_json::to_string(own_card).expect("Card serialization should not fail");

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

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterDeviceSyncState {
    /// ID of the target device.
    #[serde(with = "bytes_array_32")]
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

    /// Serializes the sync state to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("InterDeviceSyncState serialization should not fail")
    }

    /// Deserializes sync state from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }
}

/// Version vector for causality tracking across devices.
///
/// Used to detect concurrent updates and determine if changes
/// happened before, after, or concurrently with other changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionVector {
    /// Map of device ID to version number.
    #[serde(with = "version_map_serde")]
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

    /// Serializes the version vector to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("VersionVector serialization should not fail")
    }

    /// Deserializes version vector from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceSyncError> {
        serde_json::from_str(json).map_err(|e| DeviceSyncError::Deserialization(e.to_string()))
    }
}

/// Serde helper for 32-byte arrays.
mod bytes_array_32 {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
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

/// Serde helper for HashMap<[u8; 32], u64> using hex-encoded keys.
mod version_map_serde {
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};
    use std::collections::HashMap;
    use std::fmt;

    pub fn serialize<S>(map: &HashMap<[u8; 32], u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut ser_map = serializer.serialize_map(Some(map.len()))?;
        for (key, value) in map {
            ser_map.serialize_entry(&hex::encode(key), value)?;
        }
        ser_map.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<[u8; 32], u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VersionMapVisitor;

        impl<'de> Visitor<'de> for VersionMapVisitor {
            type Value = HashMap<[u8; 32], u64>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map with hex-encoded 32-byte keys and u64 values")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut map = HashMap::new();
                while let Some((key, value)) = access.next_entry::<String, u64>()? {
                    let bytes = hex::decode(&key).map_err(serde::de::Error::custom)?;
                    let arr: [u8; 32] = bytes
                        .try_into()
                        .map_err(|_| serde::de::Error::custom("invalid key length"))?;
                    map.insert(arr, value);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_map(VersionMapVisitor)
    }
}
