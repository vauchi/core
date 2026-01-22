//! Persistent Storage Module
//!
//! Provides encrypted local storage for contacts, identity, and sync state.
//! Uses SQLite with application-level encryption for sensitive data.

#[cfg(feature = "testing")]
pub mod contacts;
#[cfg(not(feature = "testing"))]
mod contacts;

#[cfg(feature = "testing")]
pub mod device;
#[cfg(not(feature = "testing"))]
mod device;

#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;

#[cfg(feature = "testing")]
pub mod identity;
#[cfg(not(feature = "testing"))]
mod identity;

#[cfg(feature = "testing")]
pub mod labels;
#[cfg(not(feature = "testing"))]
mod labels;

#[cfg(feature = "testing")]
pub mod pending;
#[cfg(not(feature = "testing"))]
mod pending;

#[cfg(feature = "testing")]
pub mod delivery;
#[cfg(not(feature = "testing"))]
mod delivery;

#[cfg(feature = "testing")]
pub mod retry;
#[cfg(not(feature = "testing"))]
mod retry;

#[cfg(feature = "testing")]
pub mod ratchet;
#[cfg(not(feature = "testing"))]
mod ratchet;

pub mod secure;

pub use error::{
    DeliveryRecord, DeliveryStatus, PendingUpdate, RetryEntry, RetryQueue, StorageError,
    UpdateStatus,
};
pub use secure::{FileKeyStorage, SecureStorage};

#[cfg(feature = "secure-storage")]
pub use secure::PlatformKeyring;

use rusqlite::Connection;
use std::path::Path;

use crate::crypto::SymmetricKey;

/// SQLite-based storage implementation.
///
/// Stores data in a local SQLite database with application-level encryption
/// for sensitive fields (keys, cards, etc.).
pub struct Storage {
    conn: Connection,
    /// Encryption key derived from user's master key
    pub(crate) encryption_key: SymmetricKey,
}

impl Storage {
    /// Opens or creates a storage database at the given path.
    pub fn open<P: AsRef<Path>>(
        path: P,
        encryption_key: SymmetricKey,
    ) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        let storage = Storage {
            conn,
            encryption_key,
        };
        storage.initialize_schema()?;
        Ok(storage)
    }

    /// Creates an in-memory storage (for testing).
    pub fn in_memory(encryption_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let storage = Storage {
            conn,
            encryption_key,
        };
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

            -- Identity (encrypted backup data)
            CREATE TABLE IF NOT EXISTS identity (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                backup_data_encrypted BLOB NOT NULL,
                display_name TEXT NOT NULL,
                created_at INTEGER NOT NULL
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

            -- Device info (current device)
            CREATE TABLE IF NOT EXISTS device_info (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                device_id BLOB NOT NULL,
                device_index INTEGER NOT NULL,
                device_name TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            -- Device registry (all linked devices)
            CREATE TABLE IF NOT EXISTS device_registry (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                registry_json TEXT NOT NULL,
                version INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Inter-device sync state (pending items per device)
            CREATE TABLE IF NOT EXISTS device_sync_state (
                device_id BLOB PRIMARY KEY,
                state_json TEXT NOT NULL,
                last_sync_version INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Local version vector for causality tracking
            CREATE TABLE IF NOT EXISTS version_vector (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                vector_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            -- Visibility labels
            CREATE TABLE IF NOT EXISTS visibility_labels (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                contacts_json TEXT NOT NULL DEFAULT '[]',
                visible_fields_json TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                modified_at INTEGER NOT NULL
            );

            -- Per-contact visibility overrides
            CREATE TABLE IF NOT EXISTS contact_visibility_overrides (
                contact_id TEXT NOT NULL,
                field_id TEXT NOT NULL,
                is_visible INTEGER NOT NULL,
                PRIMARY KEY (contact_id, field_id)
            );

            -- Delivery records (outbound message delivery tracking)
            CREATE TABLE IF NOT EXISTS delivery_records (
                message_id TEXT PRIMARY KEY,
                recipient_id TEXT NOT NULL,
                status TEXT NOT NULL,
                status_reason TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                expires_at INTEGER
            );

            -- Retry queue (failed deliveries awaiting retry)
            CREATE TABLE IF NOT EXISTS retry_entries (
                message_id TEXT PRIMARY KEY,
                recipient_id TEXT NOT NULL,
                payload BLOB NOT NULL,
                attempt INTEGER NOT NULL DEFAULT 0,
                next_retry INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                max_attempts INTEGER NOT NULL DEFAULT 10
            );

            -- Create indexes
            CREATE INDEX IF NOT EXISTS idx_pending_contact ON pending_updates(contact_id);
            CREATE INDEX IF NOT EXISTS idx_pending_status ON pending_updates(status);
            CREATE INDEX IF NOT EXISTS idx_label_name ON visibility_labels(name);
            CREATE INDEX IF NOT EXISTS idx_delivery_recipient ON delivery_records(recipient_id);
            CREATE INDEX IF NOT EXISTS idx_delivery_status ON delivery_records(status);
            CREATE INDEX IF NOT EXISTS idx_retry_next ON retry_entries(next_retry);
            CREATE INDEX IF NOT EXISTS idx_retry_recipient ON retry_entries(recipient_id);
            ",
        )?;
        Ok(())
    }
}
