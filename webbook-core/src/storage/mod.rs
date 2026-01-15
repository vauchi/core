//! Persistent Storage Module
//!
//! Provides encrypted local storage for contacts, identity, and sync state.
//! Uses SQLite with application-level encryption for sensitive data.

use std::path::Path;
use rusqlite::{Connection, params};
use thiserror::Error;

use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::crypto::ratchet::DoubleRatchetState;

/// Storage error types.
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),
}

/// Pending update status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Pending,
    Sending,
    Failed { error: String, retry_at: u64 },
}

/// A pending sync update.
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub id: String,
    pub contact_id: String,
    pub update_type: String,
    pub payload: Vec<u8>,
    pub created_at: u64,
    pub retry_count: u32,
    pub status: UpdateStatus,
}

/// SQLite-based storage implementation.
///
/// Stores data in a local SQLite database with application-level encryption
/// for sensitive fields (keys, cards, etc.).
pub struct Storage {
    conn: Connection,
    /// Encryption key derived from user's master key
    encryption_key: SymmetricKey,
}

impl Storage {
    /// Opens or creates a storage database at the given path.
    pub fn open<P: AsRef<Path>>(path: P, encryption_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        let storage = Storage { conn, encryption_key };
        storage.initialize_schema()?;
        Ok(storage)
    }

    /// Creates an in-memory storage (for testing).
    pub fn in_memory(encryption_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let storage = Storage { conn, encryption_key };
        storage.initialize_schema()?;
        Ok(storage)
    }

    /// Initializes the database schema.
    fn initialize_schema(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "
            -- Contacts table
            CREATE TABLE IF NOT EXISTS contacts (
                id TEXT PRIMARY KEY,
                public_key BLOB NOT NULL,
                display_name TEXT NOT NULL,
                card_encrypted BLOB NOT NULL,
                shared_key_encrypted BLOB NOT NULL,
                visibility_rules_json TEXT,
                exchange_timestamp INTEGER NOT NULL,
                fingerprint_verified INTEGER DEFAULT 0,
                last_sync_at INTEGER
            );

            -- Own contact card
            CREATE TABLE IF NOT EXISTS own_card (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                card_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Pending sync updates
            -- Note: No foreign key constraint on contact_id to allow queuing
            -- updates even before contact is fully established
            CREATE TABLE IF NOT EXISTS pending_updates (
                id TEXT PRIMARY KEY,
                contact_id TEXT NOT NULL,
                update_type TEXT NOT NULL,
                payload BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                retry_count INTEGER DEFAULT 0,
                status TEXT DEFAULT 'pending',
                error_message TEXT,
                retry_at INTEGER
            );

            -- Double Ratchet state for each contact
            CREATE TABLE IF NOT EXISTS contact_ratchets (
                contact_id TEXT PRIMARY KEY REFERENCES contacts(id),
                ratchet_state_encrypted BLOB NOT NULL,
                is_initiator INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Create indexes
            CREATE INDEX IF NOT EXISTS idx_pending_contact ON pending_updates(contact_id);
            CREATE INDEX IF NOT EXISTS idx_pending_status ON pending_updates(status);
            "
        )?;
        Ok(())
    }

    // === Contact Operations ===

    /// Saves a contact to storage.
    pub fn save_contact(&self, contact: &Contact) -> Result<(), StorageError> {
        // Serialize and encrypt the contact card
        let card_json = serde_json::to_vec(contact.card())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let card_encrypted = crate::crypto::encrypt(&self.encryption_key, &card_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Encrypt the shared key
        let shared_key_encrypted = crate::crypto::encrypt(
            &self.encryption_key,
            contact.shared_key().as_bytes(),
        ).map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Serialize visibility rules
        let visibility_json = serde_json::to_string(contact.visibility_rules())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO contacts
             (id, public_key, display_name, card_encrypted, shared_key_encrypted,
              visibility_rules_json, exchange_timestamp, fingerprint_verified, last_sync_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                contact.id(),
                contact.public_key().as_slice(),
                contact.display_name(),
                card_encrypted,
                shared_key_encrypted,
                visibility_json,
                contact.exchange_timestamp() as i64,
                contact.is_fingerprint_verified() as i32,
                Option::<i64>::None,
            ],
        )?;

        Ok(())
    }

    /// Loads a contact by ID.
    pub fn load_contact(&self, id: &str) -> Result<Option<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, exchange_timestamp, fingerprint_verified
             FROM contacts WHERE id = ?1"
        )?;

        let result = stmt.query_row(params![id], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                exchange_timestamp: row.get(6)?,
                fingerprint_verified: row.get(7)?,
            })
        });

        match result {
            Ok(row) => Ok(Some(self.row_to_contact(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Lists all contacts.
    pub fn list_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, exchange_timestamp, fingerprint_verified
             FROM contacts ORDER BY display_name"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                exchange_timestamp: row.get(6)?,
                fingerprint_verified: row.get(7)?,
            })
        })?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }

        Ok(contacts)
    }

    /// Deletes a contact by ID.
    pub fn delete_contact(&self, id: &str) -> Result<bool, StorageError> {
        // Also delete associated ratchet state
        self.conn.execute(
            "DELETE FROM contact_ratchets WHERE contact_id = ?1",
            params![id],
        )?;

        let rows_affected = self.conn.execute(
            "DELETE FROM contacts WHERE id = ?1",
            params![id],
        )?;
        Ok(rows_affected > 0)
    }

    /// Converts a database row to a Contact.
    fn row_to_contact(&self, row: ContactRow) -> Result<Contact, StorageError> {
        // Decrypt card
        let card_json = crate::crypto::decrypt(&self.encryption_key, &row.card_encrypted)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let card: ContactCard = serde_json::from_slice(&card_json)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        // Decrypt shared key
        let shared_key_bytes = crate::crypto::decrypt(&self.encryption_key, &row.shared_key_encrypted)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let shared_key_array: [u8; 32] = shared_key_bytes.try_into()
            .map_err(|_| StorageError::Encryption("Invalid key length".into()))?;
        let shared_key = SymmetricKey::from_bytes(shared_key_array);

        // Parse public key
        let public_key: [u8; 32] = row.public_key.try_into()
            .map_err(|_| StorageError::Encryption("Invalid public key length".into()))?;

        // Create contact
        let mut contact = Contact::from_exchange(public_key, card, shared_key);

        // Parse and apply visibility rules
        if let Some(json) = row.visibility_rules_json {
            let rules = serde_json::from_str(&json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            *contact.visibility_rules_mut() = rules;
        }

        // Set fingerprint verification
        if row.fingerprint_verified != 0 {
            contact.mark_fingerprint_verified();
        }

        Ok(contact)
    }

    // === Own Contact Card Operations ===

    /// Saves the user's own contact card.
    pub fn save_own_card(&self, card: &ContactCard) -> Result<(), StorageError> {
        let card_json = serde_json::to_string(card)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO own_card (id, card_json, updated_at) VALUES (1, ?1, ?2)",
            params![card_json, now as i64],
        )?;

        Ok(())
    }

    /// Loads the user's own contact card.
    pub fn load_own_card(&self) -> Result<Option<ContactCard>, StorageError> {
        let result = self.conn.query_row(
            "SELECT card_json FROM own_card WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(json) => {
                let card = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(card))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Pending Updates Operations ===

    /// Queues a pending update for a contact.
    pub fn queue_update(&self, update: &PendingUpdate) -> Result<(), StorageError> {
        let (status, error_msg, retry_at) = match &update.status {
            UpdateStatus::Pending => ("pending", None, None),
            UpdateStatus::Sending => ("sending", None, None),
            UpdateStatus::Failed { error, retry_at } => {
                ("failed", Some(error.as_str()), Some(*retry_at as i64))
            }
        };

        self.conn.execute(
            "INSERT OR REPLACE INTO pending_updates
             (id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                update.id,
                update.contact_id,
                update.update_type,
                update.payload,
                update.created_at as i64,
                update.retry_count as i32,
                status,
                error_msg,
                retry_at,
            ],
        )?;

        Ok(())
    }

    /// Gets pending updates for a contact.
    pub fn get_pending_updates(&self, contact_id: &str) -> Result<Vec<PendingUpdate>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at
             FROM pending_updates WHERE contact_id = ?1 ORDER BY created_at"
        )?;

        let rows = stmt.query_map(params![contact_id], |row| {
            let status_str: String = row.get(6)?;
            let error_msg: Option<String> = row.get(7)?;
            let retry_at: Option<i64> = row.get(8)?;

            let status = match status_str.as_str() {
                "pending" => UpdateStatus::Pending,
                "sending" => UpdateStatus::Sending,
                "failed" => UpdateStatus::Failed {
                    error: error_msg.unwrap_or_default(),
                    retry_at: retry_at.unwrap_or(0) as u64,
                },
                _ => UpdateStatus::Pending,
            };

            Ok(PendingUpdate {
                id: row.get(0)?,
                contact_id: row.get(1)?,
                update_type: row.get(2)?,
                payload: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                retry_count: row.get::<_, i32>(5)? as u32,
                status,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Gets all pending updates.
    pub fn get_all_pending_updates(&self) -> Result<Vec<PendingUpdate>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, contact_id, update_type, payload, created_at, retry_count, status, error_message, retry_at
             FROM pending_updates ORDER BY created_at"
        )?;

        let rows = stmt.query_map([], |row| {
            let status_str: String = row.get(6)?;
            let error_msg: Option<String> = row.get(7)?;
            let retry_at: Option<i64> = row.get(8)?;

            let status = match status_str.as_str() {
                "pending" => UpdateStatus::Pending,
                "sending" => UpdateStatus::Sending,
                "failed" => UpdateStatus::Failed {
                    error: error_msg.unwrap_or_default(),
                    retry_at: retry_at.unwrap_or(0) as u64,
                },
                _ => UpdateStatus::Pending,
            };

            Ok(PendingUpdate {
                id: row.get(0)?,
                contact_id: row.get(1)?,
                update_type: row.get(2)?,
                payload: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                retry_count: row.get::<_, i32>(5)? as u32,
                status,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Marks an update as sent (removes it from the queue).
    pub fn mark_update_sent(&self, update_id: &str) -> Result<bool, StorageError> {
        let rows_affected = self.conn.execute(
            "DELETE FROM pending_updates WHERE id = ?1",
            params![update_id],
        )?;
        Ok(rows_affected > 0)
    }

    /// Updates the status of a pending update.
    pub fn update_pending_status(
        &self,
        update_id: &str,
        status: UpdateStatus,
        retry_count: u32,
    ) -> Result<bool, StorageError> {
        let (status_str, error_msg, retry_at) = match &status {
            UpdateStatus::Pending => ("pending", None, None),
            UpdateStatus::Sending => ("sending", None, None),
            UpdateStatus::Failed { error, retry_at } => {
                ("failed", Some(error.as_str()), Some(*retry_at as i64))
            }
        };

        let rows_affected = self.conn.execute(
            "UPDATE pending_updates SET status = ?1, error_message = ?2, retry_at = ?3, retry_count = ?4
             WHERE id = ?5",
            params![status_str, error_msg, retry_at, retry_count as i32, update_id],
        )?;

        Ok(rows_affected > 0)
    }

    /// Counts pending updates for a contact.
    pub fn count_pending_updates(&self, contact_id: &str) -> Result<usize, StorageError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pending_updates WHERE contact_id = ?1",
            params![contact_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    // === Double Ratchet State Operations ===

    /// Saves a Double Ratchet state for a contact.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
        is_initiator: bool,
    ) -> Result<(), StorageError> {
        // Serialize the ratchet state
        let serialized = state.serialize();
        let state_json = serde_json::to_vec(&serialized)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        // Encrypt the serialized state
        let state_encrypted = crate::crypto::encrypt(&self.encryption_key, &state_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO contact_ratchets
             (contact_id, ratchet_state_encrypted, is_initiator, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                contact_id,
                state_encrypted,
                is_initiator as i32,
                now as i64,
            ],
        )?;

        Ok(())
    }

    /// Loads a Double Ratchet state for a contact.
    ///
    /// Returns the ratchet state and whether this side was the initiator.
    pub fn load_ratchet_state(
        &self,
        contact_id: &str,
    ) -> Result<Option<(DoubleRatchetState, bool)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT ratchet_state_encrypted, is_initiator FROM contact_ratchets WHERE contact_id = ?1",
            params![contact_id],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i32>(1)? != 0,
                ))
            },
        );

        match result {
            Ok((encrypted, is_initiator)) => {
                // Decrypt the state
                let state_json = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;

                // Deserialize
                let serialized: crate::crypto::ratchet::SerializedRatchetState =
                    serde_json::from_slice(&state_json)
                        .map_err(|e| StorageError::Serialization(e.to_string()))?;

                let state = DoubleRatchetState::deserialize(serialized)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;

                Ok(Some((state, is_initiator)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}

/// Internal struct for database row data.
#[allow(dead_code)]  // Fields are used via destructuring in row_to_contact
struct ContactRow {
    id: String,
    public_key: Vec<u8>,
    display_name: String,
    card_encrypted: Vec<u8>,
    shared_key_encrypted: Vec<u8>,
    visibility_rules_json: Option<String>,
    exchange_timestamp: i64,
    fingerprint_verified: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::{ContactCard, ContactField, FieldType};

    fn create_test_storage() -> Storage {
        let key = SymmetricKey::generate();
        Storage::in_memory(key).unwrap()
    }

    fn create_test_contact(name: &str) -> Contact {
        let public_key = [0u8; 32];
        let mut card = ContactCard::new(name);
        card.add_field(ContactField::new(
            FieldType::Email,
            "email",
            &format!("{}@example.com", name.to_lowercase()),
        ));
        let shared_key = SymmetricKey::generate();
        Contact::from_exchange(public_key, card, shared_key)
    }

    #[test]
    fn test_storage_save_load_contact() {
        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        let contact_id = contact.id().to_string();

        // Save
        storage.save_contact(&contact).unwrap();

        // Load
        let loaded = storage.load_contact(&contact_id).unwrap().unwrap();

        assert_eq!(loaded.id(), contact.id());
        assert_eq!(loaded.display_name(), "Alice");
        assert_eq!(loaded.card().fields().len(), 1);
    }

    #[test]
    fn test_storage_list_contacts() {
        let storage = create_test_storage();

        // Create contacts with different public keys
        let mut contact1 = create_test_contact("Alice");
        let mut contact2 = create_test_contact("Bob");

        // Give them different IDs by using different public keys
        let pk1 = [1u8; 32];
        let pk2 = [2u8; 32];
        contact1 = Contact::from_exchange(pk1, contact1.card().clone(), SymmetricKey::generate());
        contact2 = Contact::from_exchange(pk2, contact2.card().clone(), SymmetricKey::generate());

        storage.save_contact(&contact1).unwrap();
        storage.save_contact(&contact2).unwrap();

        let contacts = storage.list_contacts().unwrap();
        assert_eq!(contacts.len(), 2);
    }

    #[test]
    fn test_storage_delete_contact() {
        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        let contact_id = contact.id().to_string();

        storage.save_contact(&contact).unwrap();
        assert!(storage.load_contact(&contact_id).unwrap().is_some());

        let deleted = storage.delete_contact(&contact_id).unwrap();
        assert!(deleted);

        assert!(storage.load_contact(&contact_id).unwrap().is_none());
    }

    #[test]
    fn test_storage_contact_not_found() {
        let storage = create_test_storage();
        let result = storage.load_contact("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_storage_save_load_own_card() {
        let storage = create_test_storage();

        let mut card = ContactCard::new("My Card");
        card.add_field(ContactField::new(FieldType::Phone, "mobile", "+1234567890"));

        storage.save_own_card(&card).unwrap();

        let loaded = storage.load_own_card().unwrap().unwrap();
        assert_eq!(loaded.display_name(), "My Card");
        assert_eq!(loaded.fields().len(), 1);
    }

    #[test]
    fn test_storage_own_card_not_found() {
        let storage = create_test_storage();
        let result = storage.load_own_card().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_storage_pending_updates() {
        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        storage.save_contact(&contact).unwrap();

        let update = PendingUpdate {
            id: "update-1".to_string(),
            contact_id: contact.id().to_string(),
            update_type: "card_update".to_string(),
            payload: vec![1, 2, 3, 4],
            created_at: 12345,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };

        storage.queue_update(&update).unwrap();

        let pending = storage.get_pending_updates(contact.id()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "update-1");
        assert_eq!(pending[0].payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_storage_mark_update_sent() {
        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        storage.save_contact(&contact).unwrap();

        let update = PendingUpdate {
            id: "update-1".to_string(),
            contact_id: contact.id().to_string(),
            update_type: "card_update".to_string(),
            payload: vec![1, 2, 3],
            created_at: 12345,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };

        storage.queue_update(&update).unwrap();
        assert_eq!(storage.count_pending_updates(contact.id()).unwrap(), 1);

        storage.mark_update_sent("update-1").unwrap();
        assert_eq!(storage.count_pending_updates(contact.id()).unwrap(), 0);
    }

    #[test]
    fn test_storage_update_status() {
        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        storage.save_contact(&contact).unwrap();

        let update = PendingUpdate {
            id: "update-1".to_string(),
            contact_id: contact.id().to_string(),
            update_type: "card_update".to_string(),
            payload: vec![1, 2, 3],
            created_at: 12345,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };

        storage.queue_update(&update).unwrap();

        // Update to failed status
        storage.update_pending_status(
            "update-1",
            UpdateStatus::Failed {
                error: "Connection failed".to_string(),
                retry_at: 99999,
            },
            1,
        ).unwrap();

        let pending = storage.get_pending_updates(contact.id()).unwrap();
        assert_eq!(pending[0].retry_count, 1);
        assert!(matches!(pending[0].status, UpdateStatus::Failed { .. }));
    }

    #[test]
    fn test_storage_save_load_ratchet_state() {
        use crate::crypto::ratchet::DoubleRatchetState;
        use crate::crypto::SymmetricKey;
        use crate::exchange::X3DHKeyPair;

        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        storage.save_contact(&contact).unwrap();

        // Create ratchet state (as initiator)
        let shared_secret = SymmetricKey::generate();
        let their_dh = X3DHKeyPair::generate();
        let ratchet = DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key());

        // Save ratchet state
        storage.save_ratchet_state(contact.id(), &ratchet, true).unwrap();

        // Load ratchet state
        let (loaded, is_initiator) = storage.load_ratchet_state(contact.id()).unwrap().unwrap();

        assert!(is_initiator);
        assert_eq!(loaded.dh_generation(), ratchet.dh_generation());
        assert_eq!(loaded.our_public_key(), ratchet.our_public_key());
    }

    #[test]
    fn test_storage_ratchet_state_encryption() {
        use crate::crypto::ratchet::DoubleRatchetState;
        use crate::crypto::SymmetricKey;
        use crate::exchange::X3DHKeyPair;

        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        storage.save_contact(&contact).unwrap();

        let shared_secret = SymmetricKey::generate();
        let their_dh = X3DHKeyPair::generate();
        let mut ratchet = DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key());

        // Encrypt a message to advance the ratchet
        let _msg = ratchet.encrypt(b"test message").unwrap();

        // Save and load
        storage.save_ratchet_state(contact.id(), &ratchet, true).unwrap();
        let (mut loaded, _) = storage.load_ratchet_state(contact.id()).unwrap().unwrap();

        // The loaded ratchet should be able to continue encrypting
        let msg2 = loaded.encrypt(b"another message").unwrap();
        assert!(!msg2.ciphertext.is_empty());
    }

    #[test]
    fn test_storage_ratchet_deleted_with_contact() {
        use crate::crypto::ratchet::DoubleRatchetState;
        use crate::crypto::SymmetricKey;
        use crate::exchange::X3DHKeyPair;

        let storage = create_test_storage();
        let contact = create_test_contact("Alice");
        let contact_id = contact.id().to_string();
        storage.save_contact(&contact).unwrap();

        let shared_secret = SymmetricKey::generate();
        let their_dh = X3DHKeyPair::generate();
        let ratchet = DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key());

        storage.save_ratchet_state(&contact_id, &ratchet, true).unwrap();

        // Verify ratchet exists
        assert!(storage.load_ratchet_state(&contact_id).unwrap().is_some());

        // Delete contact
        storage.delete_contact(&contact_id).unwrap();

        // Ratchet should also be deleted
        assert!(storage.load_ratchet_state(&contact_id).unwrap().is_none());
    }

    #[test]
    fn test_storage_ratchet_not_found() {
        let storage = create_test_storage();
        let result = storage.load_ratchet_state("nonexistent").unwrap();
        assert!(result.is_none());
    }
}
