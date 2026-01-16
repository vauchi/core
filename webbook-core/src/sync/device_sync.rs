//! Device-to-Device Sync Module
//!
//! Handles syncing data between devices belonging to the same identity.
//! Used during device linking and for ongoing inter-device synchronization.

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
}
