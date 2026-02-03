// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent Storage Module
//!
//! Provides encrypted local storage for contacts, identity, and sync state.
//! Uses SQLite with application-level encryption for sensitive data.

#[cfg(feature = "testing")]
pub mod consent;
#[cfg(not(feature = "testing"))]
mod consent;

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
pub mod device_delivery;
#[cfg(not(feature = "testing"))]
mod device_delivery;

#[cfg(feature = "testing")]
pub mod ratchet;
#[cfg(not(feature = "testing"))]
mod ratchet;

#[cfg(feature = "testing")]
pub mod validation;
#[cfg(not(feature = "testing"))]
mod validation;

#[cfg(feature = "testing")]
pub mod recovery;
#[cfg(not(feature = "testing"))]
mod recovery;

#[cfg(feature = "testing")]
pub mod ux;
#[cfg(not(feature = "testing"))]
mod ux;

pub mod migration;
pub mod secure;

pub use error::{
    DeletionState, DeliveryRecord, DeliveryStatus, DeliverySummary, DeviceDeliveryRecord,
    DeviceDeliveryStatus, OfflineQueue, PendingUpdate, RetryEntry, RetryQueue, StorageError,
    UpdateStatus,
};
pub use secure::{FileKeyStorage, SecureStorage};

#[cfg(any(test, feature = "testing"))]
pub use secure::MemoryKeyStorage;

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
        Self::configure_pragmas(&conn)?;
        let storage = Storage {
            conn,
            encryption_key,
        };
        storage.run_migrations()?;
        Ok(storage)
    }

    /// Creates an in-memory storage (for testing).
    pub fn in_memory(encryption_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::configure_pragmas(&conn)?;
        let storage = Storage {
            conn,
            encryption_key,
        };
        storage.run_migrations()?;
        Ok(storage)
    }

    /// Configures SQLite PRAGMAs for performance and security.
    ///
    /// Performance:
    /// - WAL mode: enables concurrent reads during writes
    /// - synchronous=NORMAL: safe with WAL, better write throughput
    /// - cache_size=10000: larger page cache for query performance
    ///
    /// Security (defense-in-depth for crypto-shredding):
    /// - secure_delete=ON: overwrites deleted content with zeros
    /// - auto_vacuum=FULL: reclaims and overwrites freed pages on delete
    /// - temp_store=MEMORY: keeps temporary tables in RAM, not on disk
    ///
    /// Note: secure_delete is partially negated by WAL mode (pre-modification
    /// data persists in WAL file). The primary protection is the SMK encryption
    /// layer; these PRAGMAs are secondary defense-in-depth.
    fn configure_pragmas(conn: &Connection) -> Result<(), StorageError> {
        // auto_vacuum must be set before any tables are created (before first
        // page write), so it comes first — before journal_mode=WAL which writes
        // the database header.
        conn.execute_batch(
            "PRAGMA auto_vacuum=FULL;
             PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=10000;
             PRAGMA secure_delete=ON;
             PRAGMA temp_store=MEMORY;",
        )?;
        Ok(())
    }

    /// Runs all pending schema migrations.
    fn run_migrations(&self) -> Result<(), StorageError> {
        let migrations = migration::all_migrations();
        migration::MigrationRunner::run(&self.conn, &self.encryption_key, &migrations)
    }

    /// Returns the current schema version.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        migration::MigrationRunner::current_version(&self.conn)
    }

    /// Re-encrypts all encrypted columns from the current key to a new key.
    ///
    /// This is used during SMK migration: the database was opened with the old
    /// storage_key, and all data needs to be re-encrypted with the new SEK
    /// derived from SMK. After successful rekey, the internal encryption_key
    /// is updated to the new key.
    ///
    /// The operation runs in a single transaction for atomicity — if any step
    /// fails, all changes are rolled back and the old key remains valid.
    pub fn rekey(&mut self, new_key: SymmetricKey) -> Result<(), StorageError> {
        use crate::crypto::{decrypt, encrypt};
        use rusqlite::params;

        let old_key = &self.encryption_key;

        self.conn.execute_batch("BEGIN EXCLUSIVE TRANSACTION")?;

        let result = (|| -> Result<(), StorageError> {
            // Re-encrypt contacts: card_encrypted and shared_key_encrypted
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, card_encrypted, shared_key_encrypted FROM contacts")
                    .map_err(|e| StorageError::Migration(format!("Read contacts: {}", e)))?;

                let rows: Vec<(String, Vec<u8>, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(|e| StorageError::Migration(format!("Query contacts: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect contacts: {}", e)))?;

                for (id, card_enc, key_enc) in &rows {
                    let card_plain = decrypt(old_key, card_enc)
                        .map_err(|e| StorageError::Migration(format!("Decrypt card {}: {}", id, e)))?;
                    let key_plain = decrypt(old_key, key_enc)
                        .map_err(|e| StorageError::Migration(format!("Decrypt key {}: {}", id, e)))?;

                    let card_new = encrypt(&new_key, &card_plain)
                        .map_err(|e| StorageError::Migration(format!("Encrypt card {}: {}", id, e)))?;
                    let key_new = encrypt(&new_key, &key_plain)
                        .map_err(|e| StorageError::Migration(format!("Encrypt key {}: {}", id, e)))?;

                    self.conn.execute(
                        "UPDATE contacts SET card_encrypted = ?1, shared_key_encrypted = ?2 WHERE id = ?3",
                        params![card_new, key_new, id],
                    ).map_err(|e| StorageError::Migration(format!("Update contact {}: {}", id, e)))?;
                }
            }

            // Re-encrypt contacts: personal_notes_encrypted and avatar_encrypted (nullable)
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, personal_notes_encrypted, avatar_encrypted FROM contacts WHERE personal_notes_encrypted IS NOT NULL OR avatar_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read contact extras: {}", e)))?;

                type ContactExtras = (String, Option<Vec<u8>>, Option<Vec<u8>>);
                let rows: Vec<ContactExtras> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(|e| StorageError::Migration(format!("Query contact extras: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect contact extras: {}", e)))?;

                for (id, notes_enc, avatar_enc) in &rows {
                    let notes_new = if let Some(enc) = notes_enc {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt notes {}: {}", id, e)))?;
                        Some(encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt notes {}: {}", id, e)))?)
                    } else {
                        None
                    };

                    let avatar_new = if let Some(enc) = avatar_enc {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt avatar {}: {}", id, e)))?;
                        Some(encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt avatar {}: {}", id, e)))?)
                    } else {
                        None
                    };

                    self.conn.execute(
                        "UPDATE contacts SET personal_notes_encrypted = ?1, avatar_encrypted = ?2 WHERE id = ?3",
                        params![notes_new, avatar_new, id],
                    ).map_err(|e| StorageError::Migration(format!("Update contact extras {}: {}", id, e)))?;
                }
            }

            // Re-encrypt identity: backup_data_encrypted
            {
                let result: Result<(i64, Vec<u8>), _> = self.conn.query_row(
                    "SELECT id, backup_data_encrypted FROM identity WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                );

                if let Ok((id, backup_enc)) = result {
                    let plain = decrypt(old_key, &backup_enc)
                        .map_err(|e| StorageError::Migration(format!("Decrypt identity: {}", e)))?;
                    let new_enc = encrypt(&new_key, &plain)
                        .map_err(|e| StorageError::Migration(format!("Encrypt identity: {}", e)))?;
                    self.conn.execute(
                        "UPDATE identity SET backup_data_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    ).map_err(|e| StorageError::Migration(format!("Update identity: {}", e)))?;
                }
            }

            // Re-encrypt ratchet state: ratchet_state_encrypted
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT contact_id, ratchet_state_encrypted FROM contact_ratchets")
                    .map_err(|e| StorageError::Migration(format!("Read ratchets: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query ratchets: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect ratchets: {}", e)))?;

                for (contact_id, ratchet_enc) in &rows {
                    let plain = decrypt(old_key, ratchet_enc)
                        .map_err(|e| StorageError::Migration(format!("Decrypt ratchet {}: {}", contact_id, e)))?;
                    let new_enc = encrypt(&new_key, &plain)
                        .map_err(|e| StorageError::Migration(format!("Encrypt ratchet {}: {}", contact_id, e)))?;
                    self.conn.execute(
                        "UPDATE contact_ratchets SET ratchet_state_encrypted = ?1 WHERE contact_id = ?2",
                        params![new_enc, contact_id],
                    ).map_err(|e| StorageError::Migration(format!("Update ratchet {}: {}", contact_id, e)))?;
                }
            }

            // Re-encrypt own_card: card_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT card_json_encrypted FROM own_card WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt own_card: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt own_card: {}", e)))?;
                        self.conn.execute(
                            "UPDATE own_card SET card_json_encrypted = ?1 WHERE id = 1",
                            params![new_enc],
                        ).map_err(|e| StorageError::Migration(format!("Update own_card: {}", e)))?;
                    }
                }
            }

            // Re-encrypt device_registry: registry_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT registry_json_encrypted FROM device_registry WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt registry: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt registry: {}", e)))?;
                        self.conn.execute(
                            "UPDATE device_registry SET registry_json_encrypted = ?1 WHERE id = 1",
                            params![new_enc],
                        ).map_err(|e| StorageError::Migration(format!("Update registry: {}", e)))?;
                    }
                }
            }

            // Re-encrypt device_sync_state: state_json_encrypted
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT device_id, state_json_encrypted FROM device_sync_state WHERE state_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read device_sync: {}", e)))?;

                let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query device_sync: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect device_sync: {}", e)))?;

                for (device_id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt device_sync: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt device_sync: {}", e)))?;
                        self.conn.execute(
                            "UPDATE device_sync_state SET state_json_encrypted = ?1 WHERE device_id = ?2",
                            params![new_enc, device_id],
                        ).map_err(|e| StorageError::Migration(format!("Update device_sync: {}", e)))?;
                    }
                }
            }

            // Re-encrypt visibility_labels: contacts_json_encrypted, visible_fields_json_encrypted
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, contacts_json_encrypted, visible_fields_json_encrypted FROM visibility_labels WHERE contacts_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read labels: {}", e)))?;

                let rows: Vec<(String, Vec<u8>, Option<Vec<u8>>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(|e| StorageError::Migration(format!("Query labels: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect labels: {}", e)))?;

                for (id, contacts_enc, fields_enc) in &rows {
                    let contacts_new = if !contacts_enc.is_empty() {
                        let plain = decrypt(old_key, contacts_enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt label contacts {}: {}", id, e)))?;
                        encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt label contacts {}: {}", id, e)))?
                    } else {
                        contacts_enc.clone()
                    };

                    let fields_new = if let Some(enc) = fields_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, enc)
                                .map_err(|e| StorageError::Migration(format!("Decrypt label fields {}: {}", id, e)))?;
                            Some(encrypt(&new_key, &plain)
                                .map_err(|e| StorageError::Migration(format!("Encrypt label fields {}: {}", id, e)))?)
                        } else {
                            Some(enc.clone())
                        }
                    } else {
                        None
                    };

                    self.conn.execute(
                        "UPDATE visibility_labels SET contacts_json_encrypted = ?1, visible_fields_json_encrypted = ?2 WHERE id = ?3",
                        params![contacts_new, fields_new, id],
                    ).map_err(|e| StorageError::Migration(format!("Update label {}: {}", id, e)))?;
                }
            }

            // Re-encrypt device_info: device_info_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT device_info_encrypted FROM device_info WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt device_info: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt device_info: {}", e)))?;
                        self.conn.execute(
                            "UPDATE device_info SET device_info_encrypted = ?1 WHERE id = 1",
                            params![new_enc],
                        ).map_err(|e| StorageError::Migration(format!("Update device_info: {}", e)))?;
                    }
                }
            }

            // Re-encrypt version_vector: vector_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT vector_json_encrypted FROM version_vector WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt version_vector: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt version_vector: {}", e)))?;
                        self.conn.execute(
                            "UPDATE version_vector SET vector_json_encrypted = ?1 WHERE id = 1",
                            params![new_enc],
                        ).map_err(|e| StorageError::Migration(format!("Update version_vector: {}", e)))?;
                    }
                }
            }

            // Re-encrypt contact_sync_timestamps: last_sync_at_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT contact_id, last_sync_at_encrypted FROM contact_sync_timestamps WHERE last_sync_at_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read sync_timestamps: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query sync_timestamps: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect sync_timestamps: {}", e)))?;

                for (contact_id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt sync_ts: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt sync_ts: {}", e)))?;
                        self.conn.execute(
                            "UPDATE contact_sync_timestamps SET last_sync_at_encrypted = ?1 WHERE contact_id = ?2",
                            params![new_enc, contact_id],
                        ).map_err(|e| StorageError::Migration(format!("Update sync_ts: {}", e)))?;
                    }
                }
            }

            // Re-encrypt pending_updates: payload_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT id, payload_encrypted FROM pending_updates WHERE payload_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read pending: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query pending: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect pending: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt pending {}: {}", id, e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt pending {}: {}", id, e)))?;
                        self.conn.execute(
                            "UPDATE pending_updates SET payload_encrypted = ?1 WHERE id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update pending {}: {}", id, e)))?;
                    }
                }
            }

            // Re-encrypt retry_entries: payload_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT message_id, payload_encrypted FROM retry_entries WHERE payload_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read retry: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query retry: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect retry: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt retry {}: {}", id, e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt retry {}: {}", id, e)))?;
                        self.conn.execute(
                            "UPDATE retry_entries SET payload_encrypted = ?1 WHERE message_id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update retry {}: {}", id, e)))?;
                    }
                }
            }

            // Re-encrypt device_sync_checkpoints: items_json_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT target_device_id, items_json_encrypted FROM device_sync_checkpoints WHERE items_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read checkpoints: {}", e)))?;

                let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query checkpoints: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect checkpoints: {}", e)))?;

                for (device_id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt checkpoint: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt checkpoint: {}", e)))?;
                        self.conn.execute(
                            "UPDATE device_sync_checkpoints SET items_json_encrypted = ?1 WHERE target_device_id = ?2",
                            params![new_enc, device_id],
                        ).map_err(|e| StorageError::Migration(format!("Update checkpoint: {}", e)))?;
                    }
                }
            }

            // Re-encrypt recovery_responses: response_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT claim_id, response_encrypted FROM recovery_responses WHERE response_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read recovery: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query recovery: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect recovery: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt recovery {}: {}", id, e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt recovery {}: {}", id, e)))?;
                        self.conn.execute(
                            "UPDATE recovery_responses SET response_encrypted = ?1 WHERE claim_id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update recovery {}: {}", id, e)))?;
                    }
                }
            }

            // Re-encrypt deletion_state: state_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT state_json_encrypted FROM deletion_state WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt deletion_state: {}", e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt deletion_state: {}", e)))?;
                        self.conn.execute(
                            "UPDATE deletion_state SET state_json_encrypted = ?1 WHERE id = 1",
                            params![new_enc],
                        ).map_err(|e| StorageError::Migration(format!("Update deletion_state: {}", e)))?;
                    }
                }
            }

            // Re-encrypt sync_checkpoints: state_json_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT checkpoint_id, state_json_encrypted FROM sync_checkpoints WHERE state_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read batch_checkpoints: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query batch_checkpoints: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect batch_checkpoints: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt batch_checkpoint {}: {}", id, e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt batch_checkpoint {}: {}", id, e)))?;
                        self.conn.execute(
                            "UPDATE sync_checkpoints SET state_json_encrypted = ?1 WHERE checkpoint_id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update batch_checkpoint {}: {}", id, e)))?;
                    }
                }
            }

            // Re-encrypt field_validations: field_value_encrypted, signature_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT id, field_value_encrypted, signature_encrypted FROM field_validations")
                    .map_err(|e| StorageError::Migration(format!("Read field_validations for rekey: {}", e)))?;
                #[allow(clippy::type_complexity)]
                let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(|e| StorageError::Migration(format!("Query field_validations: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect field_validations: {}", e)))?;

                for (id, fv_enc, sig_enc) in &rows {
                    if let Some(enc) = fv_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, enc)
                                .map_err(|e| StorageError::Migration(format!("Decrypt field_value {}: {}", id, e)))?;
                            let new_enc = encrypt(&new_key, &plain)
                                .map_err(|e| StorageError::Migration(format!("Encrypt field_value {}: {}", id, e)))?;
                            self.conn.execute(
                                "UPDATE field_validations SET field_value_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update field_value {}: {}", id, e)))?;
                        }
                    }
                    if let Some(enc) = sig_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, enc)
                                .map_err(|e| StorageError::Migration(format!("Decrypt signature {}: {}", id, e)))?;
                            let new_enc = encrypt(&new_key, &plain)
                                .map_err(|e| StorageError::Migration(format!("Encrypt signature {}: {}", id, e)))?;
                            self.conn.execute(
                                "UPDATE field_validations SET signature_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update signature {}: {}", id, e)))?;
                        }
                    }
                }
            }

            // Re-encrypt ux_state: aha_tracker_json_encrypted, demo_contact_json_encrypted
            {
                let result = self.conn.query_row(
                    "SELECT id, aha_tracker_json_encrypted, demo_contact_json_encrypted FROM ux_state WHERE id = 1",
                    [],
                    |row| {
                        let id: i64 = row.get(0)?;
                        let aha: Option<Vec<u8>> = row.get(1)?;
                        let demo: Option<Vec<u8>> = row.get(2)?;
                        Ok((id, aha, demo))
                    },
                );

                if let Ok((id, aha_enc, demo_enc)) = result {
                    if let Some(enc) = aha_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, &enc)
                                .map_err(|e| StorageError::Migration(format!("Decrypt aha_tracker: {}", e)))?;
                            let new_enc = encrypt(&new_key, &plain)
                                .map_err(|e| StorageError::Migration(format!("Encrypt aha_tracker: {}", e)))?;
                            self.conn.execute(
                                "UPDATE ux_state SET aha_tracker_json_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update aha_tracker: {}", e)))?;
                        }
                    }
                    if let Some(enc) = demo_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, &enc)
                                .map_err(|e| StorageError::Migration(format!("Decrypt demo_contact: {}", e)))?;
                            let new_enc = encrypt(&new_key, &plain)
                                .map_err(|e| StorageError::Migration(format!("Encrypt demo_contact: {}", e)))?;
                            self.conn.execute(
                                "UPDATE ux_state SET demo_contact_json_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update demo_contact: {}", e)))?;
                        }
                    }
                }
            }

            // Re-encrypt audit_log: details_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT id, details_encrypted FROM audit_log WHERE details_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read audit_log for rekey: {}", e)))?;
                let rows: Vec<(i64, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query audit_log: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect audit_log: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc)
                            .map_err(|e| StorageError::Migration(format!("Decrypt audit_log {}: {}", id, e)))?;
                        let new_enc = encrypt(&new_key, &plain)
                            .map_err(|e| StorageError::Migration(format!("Encrypt audit_log {}: {}", id, e)))?;
                        self.conn.execute(
                            "UPDATE audit_log SET details_encrypted = ?1 WHERE id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update audit_log {}: {}", id, e)))?;
                    }
                }
            }

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                self.encryption_key = new_key;
                Ok(())
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Returns a reference to the underlying connection (testing only).
    #[cfg(feature = "testing")]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Returns a reference to the storage encryption key (testing only).
    #[cfg(feature = "testing")]
    pub fn key(&self) -> &SymmetricKey {
        &self.encryption_key
    }
}
